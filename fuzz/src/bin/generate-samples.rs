use std::path::Path;
use std::{fs::File, process::exit};

use fuzz::{TestAllTypes, TestEnum};
use recapn::alloc::Alloc;
use recapn::{data, message::Message, text};

fn main() {
    let env = std::env::args_os().collect::<Vec<_>>();
    if env.len() != 2 {
        eprintln!("Usage: generate-samples {{SAMPLES_DIR}}");
        exit(1);
    }

    let base_path = Path::new(&env[1]);

    macro_rules! def_samples {
        ($(
            $group:literal => [$(
                $sample:literal => $block:block,
            )+],
        )+) => {$({
            let group_path = base_path.join($group);
            std::fs::create_dir_all(&group_path).expect("failed to create sample group directory");
            $({
                let sample_path = group_path.join($sample);
                println!("Generating sample {}", sample_path.display());
                let message: Message<_> = $block;
                let file = File::create(sample_path).expect("failed to create sample file");
                if let Some(segments) = message.segments() {
                    recapn::io::write_message(file, &segments).expect("failed to write segments");
                }
            })+
        })+};
    }

    def_samples! [
        "read-all-types" => [
            "valid-all-types" => { valid_all_types() },
            "invalid-ptrs" => { wrong_ptr_types() },
        ],
    ]
}

fn valid_all_types() -> Message<'static, impl Alloc> {
    let mut message = Message::global();
    let mut b = message.builder().init_struct_root::<TestAllTypes>();
    b.bool_field().set(true);
    b.int8_field().set(!7);
    b.int16_field().set(!15);
    b.int32_field().set(!31);
    b.int64_field().set(!63);
    b.uint8_field().set(8);
    b.uint16_field().set(16);
    b.uint32_field().set(32);
    b.uint64_field().set(64);
    b.float32_field().set(32.32);
    b.float64_field().set(-64.64);
    b.text_field().set(text!("Hello fuzzer!"));
    b.data_field().set(data::Reader::from_slice(b"I'm bytes!"));
    b.enum_field().set(TestEnum::Grault);
    let mut s = b.struct_field().init();
    s.int8_field().set(i8::MIN);
    s.int16_field().set(i16::MIN);
    s.int32_field().set(i32::MIN);
    s.int64_field().set(i64::MIN);
    s.text_field().set(text!("Nested text!"));

    let _ = b.void_list().init(1024);
    let mut l = b.bool_list().init(3);
    for (i, value) in [false, true, true].into_iter().enumerate() {
        l.at(i as u32).set(value);
    }

    let mut l = b.float64_list().init(6);
    let floats = [
        f64::NEG_INFINITY,
        f64::EPSILON,
        f64::NAN,
        f64::MIN,
        f64::MAX,
        f64::INFINITY,
    ];
    for (i, value) in floats.into_iter().enumerate() {
        l.at(i as u32).set(value);
    }
    message
}

fn wrong_ptr_types() -> Message<'static, impl Alloc> {
    let mut message = Message::global();
    let mut b = message.builder().init_struct_root::<TestAllTypes>();

    // Pointer field accessors have an escape hatch that lets you access the field as any pointer.
    // We can use this to make something that's normally a text field into a struct pointer.
    let mut s = b.text_field().ptr().init_struct::<TestAllTypes>();
    s.int64_field().set(0xBEEF);

    // or make a data field into a list of bools
    let mut bits = b.data_field().ptr().init_list::<bool>(8);
    let mut value = false;
    for i in 0..8 {
        value = !value;
        bits.at(i).set(value);
    }
    message
}
