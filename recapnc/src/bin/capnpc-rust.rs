use std::fs;

use anyhow::{Result, Context};

use quote::ToTokens;
use recapn::message::{Reader, ReaderOptions};
use recapn::io::{self, StreamOptions};
use recapnc::generator::GeneratorContext;
use recapnc::gen::capnp_schema_capnp::CodeGeneratorRequest;
use recapnc::quotes::GeneratedRoot;

fn main() -> Result<()> {
    let mut stdin = std::io::stdin().lock();
    let message = io::read_from_stream(&mut stdin, StreamOptions::default())?;
    let reader = Reader::new(&message, ReaderOptions::default());
    let request = reader.read_as::<CodeGeneratorRequest>();

    let context = GeneratorContext::new(&request)?;

    let mut root_mod = GeneratedRoot { files: Vec::new() };

    for file in request.requested_files() {
        let (file, root) = context.generate_file(file.id())?;

        let parsed = syn::parse2::<syn::File>(file.to_token_stream())
            .context("parsing generated file")?;
        let printable = prettyplease::unparse(&parsed);

        fs::write(&root.path, printable)?;

        root_mod.files.push(root);
    }

    let parsed = syn::parse2::<syn::File>(root_mod.to_token_stream()).context("parsing generated file")?;
    let printable = prettyplease::unparse(&parsed);
    fs::write("mod.rs", printable)?;

    Ok(())
}