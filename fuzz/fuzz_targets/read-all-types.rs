#![no_main]

use std::error::Error;

use libfuzzer_sys::arbitrary::{self, Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;

use recapn::alloc::Word;
use recapn::list::Fallible;
use recapn::ReaderOf;
use fuzz::TestAllTypes;
use recapn::message::{Reader, ReaderOptions};

fn traverse(r: &ReaderOf<TestAllTypes>) -> recapn::Result<()> {
    macro_rules! read_data {
        ($($ident:ident,)*) => {
            $(let _ = r.$ident();)*
        };
    }

    read_data! {
        void_field,
        bool_field,
        int8_field,
        int16_field,
        int32_field,
        int64_field,
        uint8_field,
        uint16_field,
        uint32_field,
        uint64_field,
        float32_field,
        float64_field,
        enum_field,
    };

    let _ = r.text_field().try_get()?;
    let _ = r.data_field().try_get()?;
    let _ = r.struct_field().try_get()?;

    macro_rules! read_list_data {
        ($($ident:ident,)*) => {
            $(let _ = r.$ident().try_get();)*
        };
    }

    read_list_data! {
        void_list,
        bool_list,
        int8_list,
        int16_list,
        int32_list,
        int64_list,
        uint8_list,
        uint16_list,
        uint32_list,
        uint64_list,
        float32_list,
        float64_list,
        enum_list,
    };

    let text_list = r.text_list().try_get()?;
    for text in text_list.into_iter_by(Fallible) {
        let _ = text?;
    }
    
    let data_list = r.data_list().try_get()?;
    for data in data_list.into_iter_by(Fallible) {
        let _ = data?;
    }

    let struct_list = r.struct_list().try_get()?;
    for _ in struct_list {}

    Ok(())
}

fn fuzz(words: &[Word]) -> Result<(), Box<dyn Error>> {
    let (message, remainder) = recapn::io::read_from_slice(words)?;
    assert!(remainder.len() < words.len()); // make sure we actually read something from the slice

    let reader = Reader::new(&message, ReaderOptions {
        traversal_limit: 4 * 1024,
        ..ReaderOptions::DEFAULT
    });
    let root = reader.read_as_struct::<TestAllTypes>();
    root.as_ref().total_size()?;

    traverse(&root)?;

    Ok(())
}

#[derive(Debug)]
pub struct ArbitraryWords(Box<[Word]>);
impl<'a> Arbitrary<'a> for ArbitraryWords {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let len = u.len().div_ceil(8);
        let mut words = vec![Word::NULL; len].into_boxed_slice();
        u.fill_buffer(Word::slice_to_bytes_mut(&mut words))?;
        Ok(Self(words))
    }
}

fuzz_target!(|words: ArbitraryWords| {
    let _ = fuzz(&words.0);
});
