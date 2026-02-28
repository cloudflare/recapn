use std::{process::ExitCode, time::Instant};

use calculator::FunctionServer;
use recapn::ty::Capability;
use recapn_rpc::server::CallContext;

// An implementation of the Function interface wrapping pow().  Note that
// we're implementing this on the client side and will pass a reference to
// the server. The server will then be able to make calls back to the client.
struct PowerFunction;
impl FunctionServer for PowerFunction {
    fn call(
        &mut self,
        ctx: recapn_rpc::server::CallContext<
            calculator::function::CallParams,
            calculator::function::CallResults,
        >,
    ) -> recapn_rpc::server::CallResult {
        let CallContext { params, response, .. } = ctx;
        response.respond_with(|response| {
            let params = params.reader();
            let params = params.get().params().try_get()?;
            if params.len() != 2 {
                return Err(calculator::bad_params_err())
            }

            response.results().value().set(params.at(0).powf(params.at(1)));
            Ok(())
        })
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

    let (read, write) = tokio::net::TcpStream::connect(addr).await.expect("failed to bind to address").into_split();
    let (bootstrap, _) = recapn_rpc::twoparty::connect(
        read,
        write,
        recapn_rpc::Client::null(),
        Default::default(),
        Default::default(),
    );

    let calculator = calculator::Client::from_client(bootstrap);
    use_calculator(calculator).await.expect("failed to use calculator");

    ExitCode::SUCCESS
}

async fn use_calculator(calculator: calculator::Client) -> recapn_rpc::Result<()> {
    {
        // Make a request that just evaluates the literal value 123.
        //
        // What's interesting here is that evaluate() returns a "Value", which is
        // another interface and therefore points back to an object living on the
        // server.  We then have to call read() on that object to read it.
        // However, even though we are making two RPC's, this block executes in
        // *one* network round trip because of promise pipelining:  we do not wait
        // for the first call to complete before we send the second call to the
        // server.

        print!("Evaluating a literal...");

        let mut request = calculator.evaluate();
        request.params().expression().init().literal().set(123.);

        // Create a pipeline for the results. In C++ you'd call send which would give you a promise
        // for the results *and* a pipeline in the same object. But Rust is not C++, so we can't do
        // that... Instead, you specify whether you want the response, a pipeline, or both.
        // If the capability is already resolved to an error, performing a send operation will
        // immediately return that error.
        let pipeline = request.pipeline()?;

        // This will take our pipelined value capability and call read on it, returning the value.
        let start = Instant::now();
        let value = calculator::read_value(&pipeline.value()).await?;

        assert_eq!(value, 123.);

        println!(" PASS - {}ns", start.elapsed().as_nanos());
    }

    {
        // Make a request to evaluate 123 + 45 - 67.
        //
        // The Calculator interface requires that we first call getOperator() to
        // get the addition and subtraction functions, then call evaluate() to use
        // them.  But, once again, we can get both functions, call evaluate(), and
        // then read() the result -- four RPCs -- in the time of *one* network
        // round trip, because of promise pipelining.

        print!("Using add and subtract...");

        let add = {
            // Get the "add" function from the server.
            let mut request = calculator.get_operator();
            request.params().op().set(calculator::Operator::Add);
            request.pipeline()?.func()
        };

        let subtract = {
            // Get the "subtract" function from the server.
            let mut request = calculator.get_operator();
            request.params().op().set(calculator::Operator::Subtract);
            request.pipeline()?.func()
        };

        // Build the request to evaluate 123 + 45 - 67.
        let mut request = calculator.evaluate();
        let mut subtract_call = request.params().into_expression().init().into_call().set();
        subtract_call.function().set(subtract);
        let mut subtract_params = subtract_call.params().init(2);

        let mut add_call = subtract_params.at(0).get().into_call().set();
        add_call.function().set(add);
        let mut add_params = add_call.params().init(2);
        add_params.at(0).get().literal().set(123.);
        add_params.at(1).get().literal().set(45.);

        subtract_params.at(1).get().literal().set(67.);

        // Send the evaluate() request, read() the result, and wait for read() to
        // finish.
        let start = Instant::now();
        let response = calculator::read_value(&request.pipeline()?.value()).await?;
        assert_eq!(response, 101.);

        println!(" PASS - {}ns", start.elapsed().as_nanos());
    }

    {
        // Make a request to evaluate 4 * 6, then use the result in two more
        // requests that add 3 and 5.
        //
        // Since evaluate() returns its result wrapped in a `Value`, we can pass
        // that `Value` back to the server in subsequent requests before the first
        // `evaluate()` has actually returned.  Thus, this example again does only
        // one network round trip.

        print!("Pipelining eval() calls... ");

        let add = {
            // Get the "add" function from the server.
            let mut request = calculator.get_operator();
            request.params().op().set(calculator::Operator::Add);
            request.pipeline()?.func()
        };

        let multiply = {
            // Get the "subtract" function from the server.
            let mut request = calculator.get_operator();
            request.params().op().set(calculator::Operator::Multiply);
            request.pipeline()?.func()
        };

        // Build the request to evaluate 4 * 6
        let mut request = calculator.evaluate();
        let mut multiply_call = request.params().into_expression().init().into_call().set();
        multiply_call.function().set(multiply);
        let mut multiply_params = multiply_call.params().init(2);
        multiply_params.at(0).get().literal().set(4.);
        multiply_params.at(1).get().literal().set(6.);

        let multiply_result = request.pipeline()?.value();

        // Use the result in two calls that add 3 and add 5.

        let mut add3_request = calculator.evaluate();
        let mut add3_call = add3_request.params().into_expression().init().into_call().set();
        add3_call.function().set(add.clone());
        let mut add3_params = add3_call.params().init(2);
        add3_params.at(0).get().previous_result().set(multiply_result.clone());
        add3_params.at(1).get().literal().set(3.);
        let add3_value = add3_request.pipeline()?.value();

        let mut add5_request = calculator.evaluate();
        let mut add5_call = add5_request.params().into_expression().init().into_call().set();
        add5_call.function().set(add.clone());
        let mut add5_params = add5_call.params().init(2);
        add5_params.at(0).get().previous_result().set(multiply_result.clone());
        add5_params.at(1).get().literal().set(5.);
        let add5_value = add5_request.pipeline()?.value();

        let start = Instant::now();
        let add3_result = calculator::read_value(&add3_value).await?;
        let add5_result = calculator::read_value(&add5_value).await?;

        // Now wait for the results.
        assert_eq!(add3_result, 27.);
        assert_eq!(add5_result, 29.);

        println!(" PASS - {}ns", start.elapsed().as_nanos());
    }

    {
        // Our calculator interface supports defining functions.  Here we use it
        // to define two functions and then make calls to them as follows:
        //
        //   f(x, y) = x * 100 + y
        //   g(x) = f(x, x + 1) * 2;
        //   f(12, 34)
        //   g(21)
        //
        // Once again, the whole thing takes only one network round trip.

        print!("Defining functions...");

        let add = {
            // Get the "add" function from the server.
            let mut request = calculator.get_operator();
            request.params().op().set(calculator::Operator::Add);
            request.pipeline()?.func()
        };

        let multiply = {
            // Get the "multiply" function from the server.
            let mut request = calculator.get_operator();
            request.params().op().set(calculator::Operator::Multiply);
            request.pipeline()?.func()
        };

        let f = {
            // Define f.
            let mut request = calculator.def_function();
            let mut params = request.params();
            params.param_count().set(2);

            {
                // Build the function body.
                let mut add_call = params.into_body().init().into_call().set();
                add_call.function().set(add.clone());

                let mut add_params = add_call.params().init(2);
                let mut multiply_call = add_params.at(0).get().into_call().set();
                multiply_call.function().set(multiply.clone());
                let mut multiply_params = multiply_call.params().init(2);
                multiply_params.at(0).get().parameter().set(0); // x
                multiply_params.at(1).get().literal().set(100.);

                add_params.at(1).get().parameter().set(1); // y
            }

            request.pipeline()?.func()
        };

        let g = {
            // Define g.
            let mut request = calculator.def_function();
            let mut params = request.params();
            params.param_count().set(1);

            {
                // Build the function body.
                let mut multiply_call = params.into_body().init().into_call().set();
                multiply_call.function().set(multiply);
                let mut multiply_params = multiply_call.params().init(2);

                let mut f_call = multiply_params.at(0).get().into_call().set();
                f_call.function().set(f.clone());
                let mut f_params = f_call.params().init(2);
                f_params.at(0).get().parameter().set(0);

                let mut add_call = f_params.at(1).get().into_call().set();
                add_call.function().set(add);
                let mut add_params = add_call.params().init(2);
                add_params.at(0).get().parameter().set(0);
                add_params.at(1).get().literal().set(1.);

                multiply_params.at(1).get().literal().set(2.);
            }

            request.pipeline()?.func()
        };

        // OK, we've defined all our functions.  Now create our eval requests.

        // f(12, 34)
        let mut f_eval = calculator.evaluate();
        let mut f_call = f_eval.params().into_expression().init().into_call().set();
        f_call.function().set(f);
        let mut f_params = f_call.params().init(2);
        f_params.at(0).get().literal().set(12.);
        f_params.at(1).get().literal().set(34.);
        let f_value = f_eval.pipeline()?.value();

        // g(21)
        let mut g_eval = calculator.evaluate();
        let mut g_call = g_eval.params().into_expression().init().into_call().set();
        g_call.function().set(g);
        let mut g_params = g_call.params().init(1);
        g_params.at(0).get().literal().set(21.);
        let g_value = g_eval.pipeline()?.value();

        // Wait for the results.
        let start = Instant::now();
        assert_eq!(calculator::read_value(&f_value).await?, 1234.);
        assert_eq!(calculator::read_value(&g_value).await?, 4244.);

        println!(" PASS - {}ns", start.elapsed().as_nanos());
    }

    {
        // Make a request that will call back to a function defined locally.
        //
        // Specifically, we will compute 2^(4 + 5).  However, exponent is not
        // defined by the Calculator server.  So, we'll implement the Function
        // interface locally and pass it to the server for it to use when
        // evaluating the expression.
        //
        // This example requires two network round trips to complete, because the
        // server calls back to the client once before finishing.  In this
        // particular case, this could potentially be optimized by using a tail
        // call on the server side -- see CallContext::tailCall().  However, to
        // keep the example simpler, we haven't implemented this optimization in
        // the sample server.

        print!("Using a callback...");

        let add = {
            // Get the "add" function from the server.
            let mut request = calculator.get_operator();
            request.params().op().set(calculator::Operator::Add);
            request.pipeline()?.func()
        };

        // Build the eval request for 2^(4+5).
        let mut request = calculator.evaluate();
        let mut pow_call = request.params().into_expression().init().into_call().set();
        pow_call.function().set(calculator::spawn_server(PowerFunction));
        let mut pow_params = pow_call.params().init(2);
        pow_params.at(0).get().literal().set(2.);

        let mut add_call = pow_params.at(1).get().into_call().set();
        add_call.function().set(add);
        let mut add_params = add_call.params().init(2);
        add_params.at(0).get().literal().set(4.);
        add_params.at(1).get().literal().set(5.);

        // Send the request and wait.
        let start = Instant::now();
        let value = calculator::read_value(&request.pipeline()?.value()).await?;
        assert_eq!(value, 512.);

        println!(" PASS - {}ns", start.elapsed().as_nanos());
    }

    Ok(())
}