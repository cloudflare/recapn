use anyhow::{Result, Context};

use quote::ToTokens;
use recapn::message::{Reader, ReaderOptions, StreamOptions, SegmentSet};
use recapnc::generator::GeneratorContext;
use recapnc::gen::capnp_schema_capnp::CodeGeneratorRequest;

fn main() -> Result<()> {
    let mut stdin = std::io::stdin().lock();
    let message = SegmentSet::from_read(&mut stdin, StreamOptions::default())?;
    let reader = Reader::new(message, ReaderOptions::default());
    let request = reader.root().read_as_struct::<CodeGeneratorRequest>();

    let context = GeneratorContext::new(&request)?;

    for file in request.requested_files() {
        let tokens = context.generate_file(file.id())?.into_token_stream();
        let parsed = syn::parse2::<syn::File>(tokens)
            .with_context(|| format!("parsing generated file"))?;
        let printable = prettyplease::unparse(&parsed);
        println!("{}", printable);
    }

    Ok(())
}