$files = @(
    "capnp/rpc.capnp"
)

try {
    Push-Location $PSScriptRoot
    $command = "capnp compile $($files -join ' ') -o- | cargo run -p recapnc --bin capnpc-rust"
    cmd /c $command
}
finally {
    Pop-Location
}