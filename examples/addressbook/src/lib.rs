pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/mod.rs"));
}

use std::io::Write;

use generated::addressbook_capnp::*;
use recapn::io::{self, StreamOptions};
use recapn::message::{self, Message, ReaderOptions};
use recapn::text;

pub fn write_address_book<W: Write>(target: &mut W) {
    let mut message = Message::global();
    let mut builder = message.builder();
    let mut addressbook = builder.by_ref().init_struct_root::<AddressBook>();

    let mut people = addressbook.people().init(2);

    let mut alice = people.at(0).get();
    alice.id().set(123);
    alice.name().set(text!("Alice"));
    alice.email().set(text!("alice@example.com"));
    alice.employment().school().set_str("MIT");

    let mut alice_phones = alice.phones().init(1);
    let mut phone = alice_phones.at(0).get();
    phone.number().set(text!("555-1212"));
    phone.r#type().set(person::phone_number::Type::Mobile);

    let mut bob = people.at(1).get();
    bob.id().set(456);
    bob.name().set(text!("Bob"));
    bob.email().set(text!("bob@example.com"));

    let mut bob_phones = bob.phones().init(2);

    let mut home = bob_phones.at(0).get();
    home.number().set(text!("555-4567"));
    home.r#type().set(person::phone_number::Type::Home);

    let mut work = bob_phones.at(1).get();
    work.number().set(text!("555-7654"));
    work.r#type().set(person::phone_number::Type::Work);

    bob.employment().unemployed().set();

    recapn::io::write_message_packed(target, &builder.segments()).unwrap();
}

pub fn print_address_book<W: Write>(src: &[u8], output: &mut W) -> ::recapn::Result<()> {
    let segments = io::read_from_packed_stream(src, StreamOptions::default()).unwrap();
    let message = message::Reader::new(&segments, ReaderOptions::default());
    let address_book = message.read_as_struct::<AddressBook>();

    for person in address_book.people() {
        writeln!(
            output,
            "{}: {}",
            person.name().as_str().unwrap(),
            person.email().as_str().unwrap()
        )
        .unwrap();
        for phone in person.phones() {
            let type_name = match phone.r#type() {
                Ok(person::phone_number::Type::Mobile) => "mobile",
                Ok(person::phone_number::Type::Home) => "home",
                Ok(person::phone_number::Type::Work) => "work",
                Err(::recapn::NotInSchema(_)) => "UNKNOWN",
            };
            writeln!(
                output,
                "  {} phone: {}",
                type_name,
                phone.number().as_str().unwrap()
            )
            .unwrap();
        }
        match person.employment().which() {
            Ok(person::employment::Which::Unemployed(())) => {
                writeln!(output, "  unemployed").unwrap();
            }
            Ok(person::employment::Which::Employer(employer)) => {
                writeln!(output, "  employer: {}", employer.as_str().unwrap()).unwrap();
            }
            Ok(person::employment::Which::School(school)) => {
                writeln!(output, "  student at: {}", school.as_str().unwrap()).unwrap();
            }
            Ok(person::employment::Which::SelfEmployed(())) => {
                writeln!(output, "  self-employed").unwrap();
            }
            Err(_) => {}
        }
    }
    Ok(())
}

#[test]
fn test() {
    let mut t: Vec<u8> = vec![];
    write_address_book(&mut t);

    let mut string_repr: Vec<u8> = vec![];

    print_address_book(&t, &mut string_repr).unwrap();

    assert_eq!(
        String::from_utf8_lossy(&string_repr),
        "\
Alice: alice@example.com
  mobile phone: 555-1212
  student at: MIT
Bob: bob@example.com
  home phone: 555-4567
  work phone: 555-7654
  unemployed
"
    );
}
