$files = @(
    "capnp/test.capnp"
    "capnp/test-import.capnp"
    "capnp/test-import2.capnp"
)

try {
    Push-Location $PSScriptRoot
    $command = "capnp compile $($files -join ' ') -o- | cargo run -p recapnc --bin capnpc-rust"
    cmd /c $command
}
finally {
    Pop-Location
}