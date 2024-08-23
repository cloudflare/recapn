extern crate self as recapnpc;

pub mod gen;

#[doc(hidden)]
pub mod prelude {
    pub mod gen {
        pub use recapn::any::{self, AnyList, AnyPtr, AnyStruct};
        pub use recapn::data::{self, Data};
        pub use recapn::field::{
            self, Accessor, AccessorMut, Descriptor, Enum, FieldGroup, Group, Struct, UnionViewer,
            Variant, VariantDescriptor, VariantInfo, VariantMut, ViewOf, Viewable,
        };
        pub use recapn::list::{self, List};
        pub use recapn::ptr::{ElementSize, ListReader, StructBuilder, StructReader, StructSize};
        pub use recapn::rpc::{self, Capable, Table};
        pub use recapn::text::{self, Text};
        pub use recapn::ty;
        pub use recapn::{BuilderOf, Family, IntoFamily, NotInSchema, ReaderOf, Result};

        #[inline]
        pub const fn eslpr() -> ListReader<'static> {
            ListReader::empty(ElementSize::InlineComposite(StructSize::EMPTY))
        }

        #[macro_export(local_inner_macros)]
        macro_rules! generate_file {
            ($($kind:tt $name:ident $def:tt),*) => {
                $(make_type!($kind $name $def);)*
            };
        }

        #[macro_export(local_inner_macros)]
        macro_rules! make_type {
            (struct $name:ident {
                mod: $modname:ident,
                size: $size:tt,
                fields: {
                    $($descriptor:ident, $read:ident, $write:ident, $fieldty:ty = $def:tt),*$(,)?
                },
                $(union $union_def:tt,)?
                $(nested: {
                    $($nested_kind:tt $nested_name:ident $nested_def:tt),+ $(,)?
                },)?
            }) => {
                make_struct_group!($name, $modname);

                impl _p::ty::StructView for $name {
                    type Reader<'a, T: _p::Table> = $modname::Reader<'a, T>;
                    type Builder<'a, T: _p::Table> = $modname::Builder<'a, T>;
                }

                impl _p::ty::Struct for $name {
                    const SIZE: _p::StructSize = _p::StructSize $size;
                }

                impl $name {
                    $(def_field!(def : $fieldty, $descriptor $def);)*
                    $(make_union!(descriptors $union_def);)?
                }

                impl<'p, T: _p::Table + 'p> $name<_p::StructReader<'p, T>> {
                    $(def_field!(read : $fieldty, $name $descriptor $read);)*
                    $(
                        make_union!(read ($name) $union_def);
                        #[inline]
                        pub fn which(&self) -> Result<$modname::Which<&_p::StructReader<'p, T>>, _p::NotInSchema> {
                            unsafe { <$modname::Which<_> as _p::UnionViewer<_>>::get(&self.0) }
                        }
                    )?
                }

                impl<'p, T: _p::Table + 'p> $name<_p::StructBuilder<'p, T>> {
                    $(def_field!(write : $fieldty, $name $descriptor $read);)*
                    $(
                        make_union!(write ($name) $union_def);
                        #[inline]
                        pub fn which(&mut self) -> Result<$modname::Which<&mut _p::StructBuilder<'p, T>>, _p::NotInSchema> {
                            unsafe { <$modname::Which<_> as _p::UnionViewer<_>>::get(&mut self.0) }
                        }
                    )?
                }

                pub mod $modname {
                    use super::{__file, _p};

                    pub type Reader<'a, T = _p::rpc::Empty> = super::$name<_p::StructReader<'a, T>>;
                    pub type Builder<'a, T = _p::rpc::Empty> = super::$name<_p::StructBuilder<'a, T>>;

                    $(make_union!(type ($name) Which $union_def );)?

                    $($(make_type!($nested_kind $nested_name $nested_def);)+)?
                }
            };
            (group $name:ident {
                mod: $modname:ident,
                fields: {
                    $($descriptor:ident, $read:ident, $write:ident, $fieldty:ty = $def:tt),*$(,)?
                },
                $(union $union_def:tt,)?
                $(nested: {
                    $($nested_kind:tt $nested_name:ident $nested_def:tt),+ $(,)?
                },)?
            }) => {
                make_struct_group!($name, $modname);

                impl _p::ty::StructView for $name {
                    type Reader<'a, T: _p::rpc::Table> = $modname::Reader<'a, T>;
                    type Builder<'a, T: _p::rpc::Table> = $modname::Builder<'a, T>;
                }

                impl _p::FieldGroup for $name {
                    unsafe fn clear<T: _p::rpc::Table>(_: &mut _p::StructBuilder<'_, T>) {
                        std::unimplemented!()
                    }
                }

                impl $name {
                    $(def_field!(def : $fieldty, $descriptor $def);)*
                    $(make_union!(descriptors $union_def);)?
                }

                impl<'p, T: _p::Table + 'p> $name<_p::StructReader<'p, T>> {
                    $(def_field!(read : $fieldty, $name $descriptor $read);)*
                    $(
                        make_union!(read ($name) $union_def);
                        #[inline]
                        pub fn which(&self) -> Result<$modname::Which<&_p::StructReader<'p, T>>, _p::NotInSchema> {
                            unsafe { <$modname::Which<_> as _p::UnionViewer<_>>::get(&self.0) }
                        }
                    )?
                }

                impl<'p, T: _p::Table + 'p> $name<_p::StructBuilder<'p, T>> {
                    $(def_field!(write : $fieldty, $name $descriptor $write);)*
                    $(
                        make_union!(write ($name) $union_def);
                        #[inline]
                        pub fn which(&mut self) -> Result<$modname::Which<&mut _p::StructBuilder<'p, T>>, _p::NotInSchema> {
                            unsafe { <$modname::Which<_> as _p::UnionViewer<_>>::get(&mut self.0) }
                        }
                    )?
                }

                pub mod $modname {
                    use super::{__file, _p};

                    pub type Reader<'a, T = _p::rpc::Empty> = super::$name<_p::StructReader<'a, T>>;
                    pub type Builder<'a, T = _p::rpc::Empty> = super::$name<_p::StructBuilder<'a, T>>;

                    $(make_union!(type ($name) Which $union_def );)?

                    $($(make_type!($nested_kind $nested_name $nested_def);)+)?
                }
            };
            (enum $name:ident {
            }) => {

            }
        }

        #[macro_export(local_inner_macros)]
        macro_rules! make_union {
            (type ($structty:ident) $union_name:ident {
                tag_slot: $tag_slot:literal,
                fields: {
                    $($descriptor:ident, $variant:ident, $read:ident, $write:ident, $fieldty:ty = { case: $case:literal $($tail:tt)* }),*$(,)?
                },
            }) => {
                pub enum $union_name<T: _p::Viewable = _p::Family> {
                    $($variant(_p::ViewOf<T, $fieldty>),)*
                }

                impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b _p::StructReader<'p, T>> for $union_name {
                    type View = Which<&'b _p::StructReader<'p, T>>;

                    unsafe fn get(repr: &'b _p::StructReader<'p, T>) -> Result<Self::View, _p::NotInSchema> {
                        let tag = repr.data_field::<u16>($tag_slot);
                        match tag {
                            $($case => Ok($union_name::$variant(<$fieldty as _p::field::FieldType>::accessor(repr, &super::$structty::$descriptor.field))),)*
                            unknown => Err(_p::NotInSchema(unknown)),
                        }
                    }
                }

                impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b mut _p::StructBuilder<'p, T>> for $union_name {
                    type View = Which<&'b mut _p::StructBuilder<'p, T>>;

                    unsafe fn get(repr: &'b mut _p::StructBuilder<'p, T>) -> Result<Self::View, _p::NotInSchema> {
                        let tag = repr.data_field::<u16>($tag_slot);
                        match tag {
                            $($case => Ok($union_name::$variant(<$fieldty as _p::field::FieldType>::accessor(repr, &super::$structty::$descriptor.field))),)*
                            unknown => Err(_p::NotInSchema(unknown)),
                        }
                    }
                }
            };
            (descriptors {
                tag_slot: $tag_slot:literal,
                fields: {
                    $($descriptor:ident, $variant:ident, $read:ident, $write:ident, $fieldty:ty = { case: $case:literal $($tail:tt)* }),*$(,)?
                },
            }) => {
                $(def_field!(def variant : $fieldty, $descriptor { tag: $tag_slot, case: $case $($tail)* });)*
            };
            (read ($structty:ident) {
                tag_slot: $tag_slot:literal,
                fields: {
                    $($descriptor:ident, $variant:ident, $read:ident, $write:ident, $fieldty:ty = { case: $case:literal $($tail:tt)* }),*$(,)?
                },
            }) => {
                $(def_field!(read variant : $fieldty, $structty $descriptor $read);)*
            };
            (write ($structty:ident) {
                tag_slot: $tag_slot:literal,
                fields: {
                    $($descriptor:ident, $variant:ident, $read:ident, $write:ident, $fieldty:ty = { case: $case:literal $($tail:tt)* }),*$(,)?
                },
            }) => {
                $(def_field!(write variant : $fieldty, $structty $descriptor $read);)*
            };
        }

        #[macro_export]
        macro_rules! make_struct_group {
            ($name:ident, $modname:ident) => {
                #[derive(Clone)]
                pub struct $name<T = _p::Family>(T);

                impl<T> _p::IntoFamily for $name<T> {
                    type Family = $name;
                }

                impl<T: _p::Capable> _p::Capable for $name<T> {
                    type Table = T::Table;

                    type Imbued = T::Imbued;
                    type ImbuedWith<T2: _p::rpc::Table> = $name<T::ImbuedWith<T2>>;

                    fn imbued(&self) -> &Self::Imbued {
                        self.0.imbued()
                    }

                    fn imbue_release<T2: _p::rpc::Table>(
                        self,
                        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
                    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
                        let (imbued, old) = self.0.imbue_release(new_table);
                        ($name(imbued), old)
                    }

                    fn imbue_release_into<U>(
                        &self,
                        other: U,
                    ) -> (U::ImbuedWith<Self::Table>, U::Imbued)
                    where
                        U: _p::Capable,
                        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
                    {
                        self.0.imbue_release_into(other)
                    }
                }

                impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for $modname::Reader<'a, T> {
                    type Ptr = _p::StructReader<'a, T>;
                }

                impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
                    for $modname::Reader<'a, T>
                {
                    #[inline]
                    fn from(ptr: _p::StructReader<'a, T>) -> Self {
                        $name(ptr)
                    }
                }

                impl<'a, T: _p::rpc::Table> core::convert::From<$modname::Reader<'a, T>>
                    for _p::StructReader<'a, T>
                {
                    #[inline]
                    fn from(reader: $modname::Reader<'a, T>) -> Self {
                        reader.0
                    }
                }

                impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
                    for $modname::Reader<'a, T>
                {
                    #[inline]
                    fn as_ref(&self) -> &_p::StructReader<'a, T> {
                        &self.0
                    }
                }

                impl<'a, T: _p::rpc::Table> _p::ty::StructReader for $modname::Reader<'a, T> {}

                impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for $modname::Builder<'a, T> {
                    type Ptr = _p::StructBuilder<'a, T>;
                }

                impl<'a, T: _p::rpc::Table> core::convert::From<$modname::Builder<'a, T>>
                    for _p::StructBuilder<'a, T>
                {
                    #[inline]
                    fn from(reader: $modname::Builder<'a, T>) -> Self {
                        reader.0
                    }
                }

                impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
                    for $modname::Builder<'a, T>
                {
                    #[inline]
                    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
                        &self.0
                    }
                }

                impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
                    for $modname::Builder<'a, T>
                {
                    #[inline]
                    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
                        &mut self.0
                    }
                }

                impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for $modname::Builder<'a, T> {
                    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
                        Self(ptr)
                    }
                }
            };
        }

        #[macro_export]
        macro_rules! def_field {
            (def variant : $fieldty:ty, $descriptor:ident { tag: $tag_slot:literal, case: $case:literal }) => {
                const $descriptor: _p::VariantDescriptor<$fieldty> =
                    _p::VariantDescriptor::<$fieldty> {
                        variant: _p::VariantInfo {
                            slot: $tag_slot,
                            case: $case,
                        },
                        field: (),
                    };
            };
            (def variant : $fieldty:ty, $descriptor:ident { tag: $tag_slot:literal, case: $case:literal, slot: $slot:literal, default: $default:expr }) => {
                const $descriptor: _p::VariantDescriptor<$fieldty> =
                    _p::VariantDescriptor::<$fieldty> {
                        variant: _p::VariantInfo {
                            slot: $tag_slot,
                            case: $case,
                        },
                        field: _p::Descriptor::<$fieldty> {
                            slot: $slot,
                            default: $default,
                        },
                    };
            };
            (def : $fieldty:ty, $descriptor:ident ()) => {
                const $descriptor: _p::Descriptor<$fieldty> = ();
            };
            (def : $fieldty:ty, $descriptor:ident { slot: $slot:literal, default: $default:expr }) => {
                const $descriptor: _p::Descriptor<$fieldty> = _p::Descriptor::<$fieldty> {
                    slot: $slot,
                    default: $default,
                };
            };
            (read variant : $fieldty:ty, $ty:ident $descriptor:ident $name:ident) => {
                #[inline]
                pub fn $name(&self) -> _p::Variant<'_, 'p, T, $fieldty> {
                    unsafe {
                        <$fieldty as _p::field::FieldType>::variant(&self.0, &$ty::$descriptor)
                    }
                }
            };
            (write variant : $fieldty:ty, $ty:ident $descriptor:ident $name:ident) => {
                #[inline]
                pub fn $name(&mut self) -> _p::VariantMut<'_, 'p, T, $fieldty> {
                    unsafe {
                        <$fieldty as _p::field::FieldType>::variant(&mut self.0, &$ty::$descriptor)
                    }
                }
            };
            (read : $fieldty:ty, $ty:ident $descriptor:ident $name:ident) => {
                #[inline]
                pub fn $name(&self) -> _p::Accessor<'_, 'p, T, $fieldty> {
                    unsafe {
                        <$fieldty as _p::field::FieldType>::accessor(&self.0, &$ty::$descriptor)
                    }
                }
            };
            (write : $fieldty:ty, $ty:ident $descriptor:ident $name:ident) => {
                #[inline]
                pub fn $name(&mut self) -> _p::AccessorMut<'_, 'p, T, $fieldty> {
                    unsafe {
                        <$fieldty as _p::field::FieldType>::accessor(&mut self.0, &$ty::$descriptor)
                    }
                }
            };
        }

        pub use crate::generate_file;
    }
}

pub mod generator;
pub mod quotes;
pub mod schema;


use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs};
use thiserror::Error;
use quote::ToTokens;
use recapn::io::{self, StreamOptions};
use recapn::message::{Reader, ReaderOptions};
use recapn::ReaderOf;

use gen::capnp_schema_capnp::CodeGeneratorRequest;
use generator::GeneratorContext;
use quotes::GeneratedRoot;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid cap'n proto data")]
    Recapn(#[from] recapn::Error),
    #[error("invalid cap'n proto message stream")]
    Stream(#[from] recapn::io::StreamError),
    #[error("invalid schema")]
    Schema(#[from] schema::SchemaError),
    #[error(transparent)]
    Generator(#[from] generator::GeneratorError),
    #[error("failed to write file at {}: {}", .path.display(), .error)]
    FileWrite {
        path: PathBuf,
        error: std::io::Error,
    },
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

/// Read from an existing `CodeGeneratorRequest` and write files based on the given output path.
pub fn generate_from_request(
    request: &ReaderOf<'_, CodeGeneratorRequest>,
    out: impl AsRef<Path>,
) -> Result<()> {
    let out = out.as_ref();
    let context = GeneratorContext::new(request)?;

    let mut root_mod = GeneratedRoot { files: Vec::new() };

    for file in request.requested_files() {
        let (file, root) = context.generate_file(file.id())?;

        let parsed = syn::parse2::<syn::File>(file.to_token_stream())
            .expect("generated code is valid syn file");
        let printable = prettyplease::unparse(&parsed);

        let out_path = out.join(Path::new(&root.path));
        if let Some(parent) = out_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        if let Err(err) = fs::write(&out_path, printable) {
            return Err(Error::FileWrite { path: out_path, error: err })
        }

        root_mod.files.push(root);
    }

    let parsed = syn::parse2::<syn::File>(root_mod.to_token_stream())
        .expect("generated code is valid syn file");

    let printable = prettyplease::unparse(&parsed);

    let mod_path = out.join("mod.rs");
    if let Err(error) = fs::write(&mod_path, printable) {
        return Err(Error::FileWrite { path: mod_path, error })
    }

    Ok(())
}

/// Read a `CodeGeneratorRequest` capnp message from a stream and write files based on the given output path.
pub fn generate_from_request_stream(r: impl Read, out: impl AsRef<Path>) -> Result<()> {
    let message = io::read_from_stream(r, StreamOptions::default())?;
    let reader = Reader::new(&message, ReaderOptions::default());
    let request = reader.root().read_as_struct::<CodeGeneratorRequest>();

    generate_from_request(&request, out)
}

pub struct CapnpCommand {
    files: Vec<Box<Path>>,
    src_prefixes: Vec<Box<Path>>,
    import_paths: Vec<Box<Path>>,
    standard_import: bool,
    exe_path: Option<Box<OsStr>>,
}

impl CapnpCommand {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            src_prefixes: Vec::new(),
            import_paths: Vec::new(),
            standard_import: true,
            exe_path: None,
        }
    }

    pub fn src_prefix(&mut self, p: impl AsRef<Path>) -> &mut Self {
        self.src_prefixes.push(Box::from(p.as_ref()));
        self
    }

    pub fn import_path(&mut self, p: impl AsRef<Path>) -> &mut Self {
        self.import_paths.push(Box::from(p.as_ref()));
        self
    }

    pub fn file(&mut self, p: impl AsRef<Path>) -> &mut Self {
        self.files.push(Box::from(p.as_ref()));
        self
    }

    pub fn no_standard_import(&mut self) -> &mut Self {
        self.standard_import = false;
        self
    }

    /// Returns a new Command with the exe path specified for this command.
    pub fn exe_command(&self) -> Command {
        if let Some(path) = &self.exe_path {
            Command::new(path)
        } else {
            Command::new("capnp")
        }
    }

    /// Creates the process command to execute the compiler
    pub fn to_command(&self) -> Command {
        let mut cmd = self.exe_command();

        cmd.args(["compile", "-o-"]);

        if !self.standard_import {
            cmd.arg("--no-standard-import");
        }

        cmd.args(self.import_paths.iter().map(|p| {
            let mut s = OsString::from("--import-path=");
            s.push(&**p);
            s
        }));

        cmd.args(self.src_prefixes.iter().map(|p| {
            let mut s = OsString::from("--src-prefix=");
            s.push(&**p);
            s
        }));

        cmd.args(self.files.iter().map(|p| &**p));

        cmd.env_remove("PWD");

        cmd
    }

    pub fn write_to<T: AsRef<Path>>(&self, path: T) {
        let stat = self
            .exe_command()
            .arg("--version")
            .status()
            .expect("failed to spawn capnp compiler");

        assert!(
            stat.success(),
            "`capnp --version` returned an error: {}",
            stat
        );

        let mut cmd = self
            .to_command()
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to spawn capnp compiler");
        let out = cmd
            .stdout
            .take()
            .expect("missing stdout in child capnp process");

        generate_from_request_stream(out, path.as_ref())
            .expect("failed to generate code for capnp files");

        cmd.wait().expect("capnp command failed");
    }

    pub fn write_to_out_dir(&self) {
        self.write_to(env::var_os("OUT_DIR").expect("missing OUT_DIR environment variable"))
    }
}
