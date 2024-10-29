#![no_main]

use libfuzzer_sys::fuzz_target;

use recapn::io::StreamOptions;

fuzz_target!(|data: &[u8]| {
    let _ = recapn::io::read_from_packed_stream(data, StreamOptions::default());
});
