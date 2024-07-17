fn main() -> anyhow::Result<()> {
    let mut stdin = std::io::stdin().lock();
    recapnc::generate_from_request_stream(&mut stdin, ".")
}