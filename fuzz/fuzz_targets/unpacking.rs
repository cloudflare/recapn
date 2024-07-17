#![no_main]

use libfuzzer_sys::fuzz_target;

use recapn::io::{PackedStream, StreamOptions};

fuzz_target!(|data: &[u8]| {
    let mut packed = PackedStream::new(data);
    let _ = recapn::io::read_packed_from_stream(&mut packed, StreamOptions::default());
});
