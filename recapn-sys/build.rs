use std::process::Command;

fn main() {
    let command = std::env::var_os("CAPNP_TOOL_PATH")
        .map(|p| p.into_string().unwrap())
        .unwrap_or("capnp".to_string());
    let output = Command::new(&command)
        .arg("--version")
        .output()
        .expect("capnp tool can't be executed, change PATH or CAPNP_TOOL_PATH");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let version = stdout.split_ascii_whitespace().last().unwrap();
    if !version.starts_with("1.") && version != "(unknown)" {
        panic!("capnp tool 1.x is required, got: {:?}", version);
    }
    println!("cargo::rustc-env=CAPNP_TOOL_PATH={command}");
}
