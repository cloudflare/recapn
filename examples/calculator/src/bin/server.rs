use std::{process::ExitCode, sync::Arc};

use recapn::{ReaderOf, ty::Capability};
use recapn_channel::task::DataTask;
use recapn_rpc::server::{CallContext, FromServer};

use calculator::{
    Client as Calculator, Server as CalculatorServer, Value, ValueServer, Function, FunctionServer
};
use recapn_rpc::table::CapTable;

struct CalculatorImpl;
impl CalculatorServer for CalculatorImpl {
    fn evaluate(
        &mut self,
        ctx: CallContext<calculator::EvaluateParams, calculator::EvaluateResults>,
    ) -> recapn_rpc::server::CallResult {
        let CallContext { params, response, .. } = ctx;
        response.spawn_with(|response| Box::pin(async move {
            let expression = {
                let reader = params.reader();
                let params = reader.get();
                Expression::from_proto(&params.expression().try_get()?)?
            };

            let result = expression.evaluate(&[]).await?;
            let value = calculator::spawn_server(ValueImpl(result));
            {
                let mut results = response.results();
                results.value().set(value);
            }

            Ok(())
        }))
    }
    fn def_function(
        &mut self,
        ctx: recapn_rpc::server::CallContext<
            calculator::DefFunctionParams,
            calculator::DefFunctionResults,
        >,
    ) -> recapn_rpc::server::CallResult {
        let CallContext { params, response, .. } = ctx;
        response.respond_with(|response| {
            let func = {
                let reader = params.reader();
                let params = reader.get();
                calculator::spawn_server(FunctionImpl {
                    param_len: params.param_count(),
                    body: Arc::new(Expression::from_proto(&params.body().try_get()?)?)
                })
            };
            response.results().func().set(func);
            Ok(())
        })
    }
    fn get_operator(
        &mut self,
        ctx: recapn_rpc::server::CallContext<
            calculator::GetOperatorParams,
            calculator::GetOperatorResults,
        >,
    ) -> recapn_rpc::server::CallResult {
        let CallContext { params, response, .. } = ctx;
        response.respond_with(|response| {
            let operator = {
                let reader = params.reader();
                match reader.get().op() {
                    Ok(op) => calculator::spawn_server(OperatorImpl(op)),
                    Err(_) => return Err(recapn_rpc::Error::failed("unknown operator"))
                }
            };
            response.results().func().set(operator);
            Ok(())
        })
    }
}

struct ValueImpl(f64);
impl ValueServer for ValueImpl {
    fn read(
        &mut self,
        ctx: recapn_rpc::server::CallContext<calculator::value::ReadParams, calculator::value::ReadResults>,
    ) -> recapn_rpc::server::CallResult {
        ctx.response.respond_with(|r| {
            r.results().value().set(self.0);
            Ok(())
        })
    }
}

struct OperatorImpl(calculator::Operator);
impl FunctionServer for OperatorImpl {
    fn call(
        &mut self,
        ctx: recapn_rpc::server::CallContext<
            calculator::function::CallParams,
            calculator::function::CallResults,
        >,
    ) -> recapn_rpc::server::CallResult {
        let CallContext { params, response, .. } = ctx;
        response.respond_with(|response| {
            let reader = params.reader();
            let args = reader.get().params().try_get()?;

            if args.len() != 2 {
                return Err(calculator::bad_params_err())
            }

            let arg0 = args.at(0);
            let arg1 = args.at(1);

            let result = match self.0 {
                calculator::Operator::Add => arg0 + arg1,
                calculator::Operator::Subtract => arg0 - arg1,
                calculator::Operator::Multiply => arg0 * arg1,
                calculator::Operator::Divide => arg0 / arg1,
            };

            response.results().value().set(result);
            Ok(())
        })
    }
}

struct FunctionImpl {
    param_len: i32,
    body: Arc<Expression>,
}
impl FunctionServer for FunctionImpl {
    fn call(
        &mut self,
        ctx: recapn_rpc::server::CallContext<calculator::function::CallParams, calculator::function::CallResults>,
    ) -> recapn_rpc::server::CallResult {
        let CallContext { params, response, .. } = ctx;
        let param_len = self.param_len;
        let body = self.body.clone();

        response.spawn_with(move |response| Box::pin(async move {
            let params = {
                let reader = params.reader();
                let args = reader.get().params().try_get()?;
                if args.len() as i32 != param_len {
                    return Err(calculator::bad_params_err())
                }

                args.into_iter().collect::<Vec<f64>>()
            };

            let result = body.evaluate(&params).await?;
            response.results().value().set(result);

            Ok(())
        }))
    }
}

enum Expression {
    Literal(f64),
    PreviousResult(Value),
    Parameter(u32),
    Call {
        function: Function,
        params: Vec<Expression>,
    }
}

impl Expression {
    fn from_proto(proto: &ReaderOf<'_, calculator::Expression, CapTable<'_>>) -> recapn_rpc::Result<Self> {
        use calculator::expression::Which as WhichExpression;
        let which = proto.which()
            .map_err(|_| recapn_rpc::Error::failed("invalid expression type"))?;
        Ok(match which {
            WhichExpression::Literal(value) => Self::Literal(value),
            WhichExpression::PreviousResult(value) => {
                Self::PreviousResult(value.get())
            },
            WhichExpression::Parameter(idx) => Self::Parameter(idx),
            WhichExpression::Call(call) => {
                let function = call.function().get();
                let params_proto = call.params().try_get()?;
                let mut params = Vec::with_capacity(params_proto.len() as usize);
                for param in params_proto {
                    params.push(Self::from_proto(&param)?)
                }
                Self::Call { function, params }
            },
        })
    }

    async fn evaluate(&self, params: &[f64]) -> recapn_rpc::Result<f64> {
        match self {
            &Expression::Literal(value) => Ok(value),
            Expression::PreviousResult(value) => calculator::read_value(value).await,
            &Expression::Parameter(idx) => {
                params
                    .get(idx as usize)
                    .copied()
                    .ok_or_else(|| recapn_rpc::Error::failed("index out of range"))
            },
            Expression::Call { function, params: param_exprs } => {
                let params = {
                    let mut param_values = vec![0.; param_exprs.len()];
                    let mut tasks = recapn_channel::task::DataTaskSet::new();
                    for (idx, expr) in param_exprs.iter().enumerate() {
                        tasks.insert(DataTask::new(idx, expr.evaluate(params)));
                    }
                    while let Some((data, result)) = tasks.join_next().await {
                        param_values[*data] = result?;
                    }
                    param_values
                };

                let call_req = {
                    let mut call_req = function.call();
                    let mut call_params = call_req.params();
                    let mut params_list = call_params.params().init(params.len() as u32);
                    for i in 0..params_list.len().get() {
                        params_list.at(i).set(params[i as usize]);
                    }
                    call_req
                };

                Ok(call_req.send()?.await.results()?.get().value())
            },
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args();
    let exec = args.next().unwrap();
    let eprint_usage = || {
        eprintln!("usage: {exec} ADDRESS[:PORT]");
        eprintln!("Runs the server bound to the given address/port.");
    };
    let Some(addr) = args.next() else {
        eprint_usage();
        return ExitCode::FAILURE
    };

    if args.next().is_some() {
        eprintln!("too many args");
        eprint_usage();
        return ExitCode::FAILURE
    }

    let listener = tokio::net::TcpListener::bind(addr).await.expect("failed to bind to address");
    println!("bound to {}", listener.local_addr().unwrap());
    loop {
        let (bootstrap, server) = Calculator::from_server(CalculatorImpl);
        tokio::spawn(async move {
            let mut server = server;
            server.run().await
        });
        let (conn, addr) = listener.accept().await.expect("accept() failed");
        println!("connected to {addr}");
        let (read, write) = conn.into_split();
        let (_, _) = recapn_rpc::twoparty::connect(
            read,
            write,
            bootstrap.into_inner(),
            Default::default(),
            Default::default(),
        );
    }
}