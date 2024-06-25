use std::collections::HashMap;

use anyhow::{Result, Context, ensure};
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use heck::{AsSnakeCase, AsPascalCase, AsShoutySnakeCase};
use recapn::alloc::{self, AllocLen, SegmentOffset, Word};
use recapn::ReaderOf;
use recapn::any::{AnyList, AnyStruct};
use recapn::ptr::{StructSize, UnwrapErrors};
use syn::PathSegment;
use syn::punctuated::Punctuated;

use crate::quotes::{
    GeneratedFile, GeneratedStruct, GeneratedField, GeneratedItem, GeneratedWhich, FieldDescriptor,
    GeneratedVariant, GeneratedEnum, GeneratedConst,
};
use crate::gen::capnp_schema_capnp as schema;
use schema::{Node, Field, Value, CodeGeneratorRequest, Type};
use schema::node::{Struct, Const, Which as NodeKind};
use schema::r#type::{
    Which as TypeKind,
    any_pointer::Which as AnyPtrKind,
    any_pointer::unconstrained::Which as ConstraintKind,
};

pub mod ident;

use self::ident::{ScopedIdentifierSet, Scope, IdentifierSet};

const NO_DISCRIMINANT: u16 = 0xffff;

#[derive(Debug)]
struct FileInfo {
    pub mod_ident: syn::Ident,
}

#[derive(Debug)]
struct StructInfo {
    pub type_info: TypeInfo,
    pub mod_ident: syn::Ident,
}

#[derive(Clone, Debug, Hash)]
struct ModScope {
    pub id: u64,
    pub mod_ident: syn::Ident,
}

impl PartialEq for ModScope {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for ModScope {}

type TypeScope = Scope<ModScope>;

#[derive(Debug)]
struct TypeInfo {
    /// The identifier for the type itself.
    pub type_ident: syn::Ident,
    /// The scope the type is defined in.
    pub scope: TypeScope,
}

impl TypeInfo {
    /// Resolve a path to this type from the given reference scope.
    /// 
    /// For example in the case where we want to refer to A from B, B is the ref scope we're
    /// resolving from.
    pub fn resolve_path(&self, ref_scope: &TypeScope) -> syn::Path {
        let Self { type_ident, scope } = self;
        if ref_scope == scope {
            // We're referencing this type from the same scope, so we can just use the type identifier.
            return syn::Path::from(type_ident.clone())
        }

        let mut segments: Punctuated<PathSegment, _> = Punctuated::new();
        let mut mod_path = scope.types.as_slice();
        if ref_scope.file == scope.file {
            // First, check if we're attempting to refer to a type *in* the parent scope.
            if let Some((_, ref_parent_types)) = ref_scope.types.split_last() {
                if mod_path == ref_parent_types {
                    // It's the parent scope! So we can use `super` directly instead of `__file`
                    return syn::parse_quote!(super::#type_ident)
                }
            }

            // Next, check if we're referring to something *from* a parent scope.
            // In that case, we can simply refer to the type starting from what module we're in.
            if let Some(suffix) = mod_path.strip_prefix(ref_scope.types.as_slice()) {
                mod_path = suffix;

                // todo(someday): cousin scopes? In set A(B(C), D(E)), refer to C from E
                // using super::b::C instead of __file::a::b::c. Might not be worth it.
            } else {
                // If none of those work, we can can always just use the full path from the
                // `__file` import. We're referencing this type from the same file, so we can
                // refer to it with the full path using the `__file` import.
                segments.push(syn::parse_quote!(__file));
            }
        } else {
            // We're referencing this type from a different file, so we refer to it with the full path
            // including file module using the `__imports` import.
            segments.push(syn::parse_quote!(__imports));
            segments.push(PathSegment::from(scope.file.mod_ident.clone()));
        }

        segments.extend(mod_path.iter().map(|s| PathSegment::from(s.mod_ident.clone())));
        segments.push(PathSegment::from(type_ident.clone()));

        syn::Path { leading_colon: None, segments }
    }
}

#[derive(Debug)]
struct EnumInfo {
    pub enumerants: Vec<syn::Ident>,
    pub type_info: TypeInfo,
}

#[derive(Debug)]
struct ConstInfo {
    pub ident: syn::Ident,
    pub scope: TypeScope,
}

struct NodeContext<'a> {
    pub node: ReaderOf<'a, Node>,
    pub info: Option<NodeInfo>,
}

enum FieldKind {
    Data,
    Pointer,
}

impl FieldKind {
    pub fn from_proto(r: &ReaderOf<Type>) -> Result<Self> {
        use TypeKind as Kind;
        Ok(match r.which()? {
            Kind::Void(_) |
            Kind::Bool(_) |
            Kind::Uint8(_) |
            Kind::Uint16(_) |
            Kind::Uint32(_) |
            Kind::Uint64(_) |
            Kind::Int8(_) |
            Kind::Int16(_) |
            Kind::Int32(_) |
            Kind::Int64(_) |
            Kind::Float32(_) |
            Kind::Float64(_) |
            Kind::Enum(_) => Self::Data,
            Kind::Text(_) |
            Kind::Data(_) |
            Kind::List(_) |
            Kind::Struct(_) |
            Kind::Interface(_) |
            Kind::AnyPointer(_) => Self::Pointer,
        })
    }
}

#[derive(Debug)]
enum NodeInfo {
    File(FileInfo),
    Struct(StructInfo),
    Enum(EnumInfo),
    Const(ConstInfo),
}

pub struct GeneratorContext<'a> {
    nodes: HashMap<u64, NodeContext<'a>>,
}

impl<'a> GeneratorContext<'a> {
    pub fn new(request: &ReaderOf<'a, CodeGeneratorRequest>) -> Result<Self>{
        let mut identifiers = ScopedIdentifierSet::new();
        let mut nodes = request.nodes()
            .into_iter()
            .map(|node| (node.id(), NodeContext { node, info: None }))
            .collect();

        let files = request.nodes().into_iter().filter(|node| node.file().is_set());
        for node in files {
            Self::validate_file_node(&mut nodes, &mut identifiers, &node)
                .with_context(|| format!("Failed to validate file node {}", node.id()))?;
        }

        Ok(Self { nodes })
    }

    fn validate_file_node(
        nodes: &mut HashMap<u64, NodeContext<'a>>,
        identifiers: &mut ScopedIdentifierSet<ModScope>,
        file: &ReaderOf<'a, Node>,
    ) -> Result<()> {
        let node = nodes.get_mut(&file.id())
            .with_context(|| format!("missing node {}", file.id()))?;

        ensure!(node.info.is_none(), "node already has associated info");

        let display_name = file.display_name().as_str()?;
        let mod_ident = identifiers.make_unique(None, display_name)?;
        node.info = Some(NodeInfo::File(FileInfo { mod_ident: mod_ident.clone() }));

        let mut scope = Scope::file(ModScope { id: file.id(), mod_ident });

        Self::validate_scope(file, nodes, identifiers, &mut scope)?;

        Ok(())
    }

    fn validate_scope(
        node: &ReaderOf<'a, Node>,
        nodes: &mut HashMap<u64, NodeContext<'a>>,
        identifiers: &mut ScopedIdentifierSet<ModScope>,
        scope: &mut TypeScope,
    ) -> Result<()> {
        for nested in node.nested_nodes() {
            let name = nested.name().as_str()?;
            Self::validate_node(nested.id(), name, nodes, identifiers, scope)?;
        }

        // For some reason, groups are not considered nested nodes of structs. So we
        // have to iter over all fields and find all the groups contained
        if let Some(struct_type) = node.r#struct().get() {
            let groups = struct_type
                .fields()
                .into_iter()
                .filter_map(|f| f.group().get().map(|group| (f, group.type_id())));
            for (field, group_id) in groups {
                let name = field.name().as_str()?;
                Self::validate_node(group_id, name, nodes, identifiers, scope)?;
            }
        }
        Ok(())
    }

    fn validate_node(
        node: u64,
        name: &str,
        nodes: &mut HashMap<u64, NodeContext<'a>>,
        identifiers: &mut ScopedIdentifierSet<ModScope>,
        scope: &mut TypeScope,
    ) -> Result<()> {
        let Some(NodeContext { info, ref node }) = nodes.get_mut(&node) else { return Ok(()); };

        ensure!(info.is_none(), "node already has associated info");

        match node.which()? {
            NodeKind::Struct(s) => {
                let type_ident = identifiers.make_unique(Some(scope.clone()), AsPascalCase(name))?;
                let mod_ident = identifiers.make_unique(Some(scope.clone()), AsSnakeCase(name))?;

                let node_info = StructInfo {
                    type_info: TypeInfo {
                        type_ident,
                        scope: scope.clone(),
                    },
                    mod_ident: mod_ident.clone(),
                };

                *info = Some(NodeInfo::Struct(node_info));

                scope.push(ModScope { id: node.id(), mod_ident });

                if s.discriminant_count() != 0 {
                    // Insert Which into the identifier set so if any conflicts appear they'll
                    // be made properly unique
                    let _ = identifiers.make_unique(Some(scope.clone()), "Which")?;
                }

                Self::validate_scope(&node.clone(), nodes, identifiers, scope)?;

                scope.pop();
            },
            NodeKind::Enum(e) => {
                let type_ident = identifiers.make_unique(Some(scope.clone()), AsPascalCase(name))?;

                let enumerants = {
                    let mut idents = IdentifierSet::new();
                    e.enumerants()
                        .into_iter()
                        .map(|e| {
                            let name = e.name().as_str()?;
                            let ident = idents.make_unique(AsPascalCase(name))?;
                            Ok(ident)
                        })
                        .collect::<Result<_>>()?
                };
        
                *info = Some(NodeInfo::Enum(EnumInfo {
                    enumerants,
                    type_info: TypeInfo {
                        type_ident,
                        scope: scope.clone(),
                    },
                }));
            }
            NodeKind::Const(_) => {
                *info = Some(NodeInfo::Const(ConstInfo {
                    scope: scope.clone(),
                    ident: identifiers.make_unique(Some(scope.clone()), AsShoutySnakeCase(name))?
                }))
            },
            NodeKind::File(()) => unreachable!(),
            NodeKind::Interface(_) => todo!("generate interface info"),
            NodeKind::Annotation(_) => {}, // ignored
        }

        Ok(())
    }

    pub fn generate_file(&self, id: u64) -> Result<GeneratedFile> {
        let NodeContext {
            node,
            info: Some(NodeInfo::File(FileInfo { mod_ident }))
        } = &self.nodes[&id] else { anyhow::bail!("expected file node") };

        let items = node.nested_nodes()
            .into_iter()
            .map(|nested| self.generate_item(nested.id()))
            .collect::<Result<_>>()?;
        Ok(GeneratedFile { items, ident: mod_ident.clone() })
    }

    fn generate_item(&self, id: u64) -> Result<GeneratedItem> {
        let NodeContext { node, info } = &self.nodes[&id];
        match (node.which()?, info) {
            (NodeKind::Struct(s), Some(NodeInfo::Struct(info))) => {
                Ok(GeneratedItem::Struct(self.generate_struct(node, &s, info)?))
            },
            (NodeKind::Enum(_), Some(NodeInfo::Enum(info))) => {
                Ok(GeneratedItem::Enum(self.generate_enum(info)?))
            },
            (NodeKind::Const(c), Some(NodeInfo::Const(info))) => {
                Ok(GeneratedItem::Const(self.generate_const(&c, info)?))
            }
            (NodeKind::Interface(_), None) => todo!(),
            (NodeKind::Annotation(_), None) => todo!(),
            (NodeKind::File(()), None) => unimplemented!("found nested file node inside a file"),
            _ => anyhow::bail!("missing node info"),
        }
    }

    fn generate_struct(&self, node: &ReaderOf<Node>, struct_group: &ReaderOf<Struct>, info: &StructInfo) -> Result<GeneratedStruct> {
        let (fields, variants) = struct_group.fields()
            .into_iter()
            .partition::<Vec<_>, _>(|field| field.discriminant_value() == NO_DISCRIMINANT);

        let mut discriminant_idents = IdentifierSet::new();
        let mut descriptor_idents = IdentifierSet::new();
        let mut accessor_idents = IdentifierSet::new();

        let fields = fields.into_iter().map(|field| {
            self.generate_field(&field, info, &mut descriptor_idents, &mut accessor_idents)
        }).collect::<Result<_>>()?;

        assert_eq!(struct_group.discriminant_count() as usize, variants.len());
        let which = if struct_group.discriminant_count() != 0 {
            // Create a scope used for resolving types from within the struct's mod scope.
            let struct_mod_scope = {
                let mut struct_scope = info.type_info.scope.clone();
                struct_scope.push(ModScope { id: node.id(), mod_ident: info.mod_ident.clone() });
                struct_scope
            };
            let fields = variants.into_iter().map(|field| {
                let name = field.name().as_str()?;
                let discriminant_ident = discriminant_idents.make_unique(AsPascalCase(name))?;

                let generated_field = self.generate_field(&field, info, &mut descriptor_idents, &mut accessor_idents)?;
                let variant = GeneratedVariant {
                    discriminant_ident,
                    discriminant_field_type: self.field_type(&field, &struct_mod_scope)?.0,
                    case: field.discriminant_value(),
                    field: generated_field,
                };
                Ok(variant)
            }).collect::<Result<_>>()?;
            Some(GeneratedWhich { tag_slot: struct_group.discriminant_offset(), fields, type_params: Vec::new() })
        } else {
            None
        };

        let size = (!struct_group.is_group()).then(|| StructSize {
            data: struct_group.data_word_count(),
            ptrs: struct_group.pointer_count(),
        });

        let nested_items = node.nested_nodes()
            .into_iter()
            .map(|nested| self.generate_item(nested.id()));
        let nested_groups = node.r#struct()
            .get()
            .into_iter()
            .flat_map(|node| node
                .fields()
                .into_iter()
                .filter_map(|field| field
                    .group()
                    .get()
                    .map(|group| self.generate_item(group.type_id()))
                )
            );
        let all_items = nested_items.chain(nested_groups).collect::<Result<_>>()?;

        Ok(GeneratedStruct {
            ident: info.type_info.type_ident.clone(),
            mod_ident: info.mod_ident.clone(),
            type_params: Vec::new(),
            size,
            fields,
            which,
            nested_items: all_items,
        })
    }

    fn generate_field(
        &self,
        field: &ReaderOf<Field>,
        StructInfo {
            type_info: TypeInfo {
                type_ident,
                scope,
            },
            ..
        }: &StructInfo,
        descriptor_idents: &mut IdentifierSet,
        accessor_idents: &mut IdentifierSet,
    ) -> Result<GeneratedField> {
        let name = field.name().as_str()?;
        let accessor_ident = accessor_idents.make_unique(AsSnakeCase(name))?;
        let descriptor_ident = descriptor_idents.make_unique(AsShoutySnakeCase(name))?;
        let (field_type, kind) = self.field_type(field, scope)?;
        let descriptor = self.descriptor(field, scope)?;
        let type_name = type_ident.clone();
        let own_accessor_ident = match kind { 
            FieldKind::Data => None,
            FieldKind::Pointer => Some(accessor_idents.make_unique(AsSnakeCase(format!("into_{name}")))?),
        };

        Ok(GeneratedField { type_name, field_type, own_accessor_ident, accessor_ident, descriptor_ident, descriptor })
    }

    fn field_type(&self, field: &ReaderOf<Field>, scope: &TypeScope) -> Result<(Box<syn::Type>, FieldKind)> {
        match field.which()? {
            schema::field::Which::Slot(slot) => {
                let type_info = slot.r#type().get();
                let syn_type = self.resolve_type(scope, &type_info)?;
                let kind = FieldKind::from_proto(&type_info)?;
                Ok((syn_type, kind))
            }
            schema::field::Which::Group(group) => {
                let type_name = self.resolve_type_name(scope, group.type_id())?; 
                let syn_type = Box::new(syn::parse_quote!(_p::Group<#type_name>));
                Ok((syn_type, FieldKind::Pointer))
            }
        }
    }

    fn descriptor(&self, field: &ReaderOf<Field>, scope: &TypeScope) -> Result<Option<FieldDescriptor>> {
        Ok(match field.which()? {
            schema::field::Which::Slot(slot) => {
                let type_info = slot.r#type().get();
                if type_info.void().is_set() {
                    None
                } else {
                    let default_value = slot.default_value().get_option();
                    let value = self.generate_value(scope, &type_info, default_value.as_ref())?;
                    Some(FieldDescriptor { slot: slot.offset(), default: value })
                }
            }
            schema::field::Which::Group(_) => None
        })
    }

    fn resolve_type(&self, scope: &TypeScope, info: &ReaderOf<Type>) -> Result<Box<syn::Type>> {
        Ok(Box::new(match info.which()? {
            TypeKind::Void(()) => syn::parse_quote!(()),
            TypeKind::Bool(()) => syn::parse_quote!(bool),
            TypeKind::Int8(()) => syn::parse_quote!(i8),
            TypeKind::Uint8(()) => syn::parse_quote!(u8),
            TypeKind::Int16(()) => syn::parse_quote!(i16),
            TypeKind::Uint16(()) => syn::parse_quote!(u16),
            TypeKind::Int32(()) => syn::parse_quote!(i32),
            TypeKind::Uint32(()) => syn::parse_quote!(u32),
            TypeKind::Int64(()) => syn::parse_quote!(i64),
            TypeKind::Uint64(()) => syn::parse_quote!(u64),
            TypeKind::Float32(()) => syn::parse_quote!(f32),
            TypeKind::Float64(()) => syn::parse_quote!(f64),
            TypeKind::Text(()) => syn::parse_quote!(_p::Text),
            TypeKind::Data(()) => syn::parse_quote!(_p::Data),
            TypeKind::List(list) => {
                let element_type = list.element_type().get();
                let resolved = self.resolve_type(scope, &element_type)?;
                syn::parse_quote!(_p::List<#resolved>)
            },
            TypeKind::Enum(e) => {
                let type_name = self.resolve_type_name(scope, e.type_id())?;
                syn::parse_quote!(_p::Enum<#type_name>)
            },
            TypeKind::Struct(s) => {
                let type_name = self.resolve_type_name(scope, s.type_id())?;
                syn::parse_quote!(_p::Struct<#type_name>)
            },
            TypeKind::Interface(_) => todo!("resolve interface types"),
            TypeKind::AnyPointer(ptr) => match ptr.which()? {
                AnyPtrKind::Unconstrained(unconstrained) => match unconstrained.which()? {
                    ConstraintKind::AnyKind(()) => syn::parse_quote!(_p::AnyPtr),
                    ConstraintKind::Struct(()) => syn::parse_quote!(_p::AnyStruct),
                    ConstraintKind::List(()) => syn::parse_quote!(_p::AnyList),
                    ConstraintKind::Capability(()) => syn::parse_quote!(_p::AnyPtr), // TODO
                },
                AnyPtrKind::Parameter(_) => {
                    todo!("resolve generic types")
                },
                AnyPtrKind::ImplicitMethodParameter(_) => todo!(),
            }
        }))
    }

    fn resolve_type_name(&self, ref_scope: &TypeScope, id: u64) -> Result<syn::Path> {
        let Some(info) = &self.nodes[&id].info else { anyhow::bail!("missing type info for {}", id) };
        match info {
            NodeInfo::Struct(StructInfo { type_info, .. }) |
            NodeInfo::Enum(EnumInfo { type_info, .. }) => {
                Ok(type_info.resolve_path(ref_scope))
            },
            NodeInfo::File(_) | NodeInfo::Const(_) => anyhow::bail!("unexpected node type"),
        }
    }

    fn generate_value(&self, scope: &TypeScope, type_info: &ReaderOf<Type>, value: Option<&ReaderOf<Value>>) -> Result<TokenStream> {
        macro_rules! value_or_default {
            ($field:ident) => {{
                let value = value.and_then(|v| v.$field().get()).unwrap_or_default();
                quote!(#value)
            }};
        }

        let expr = match type_info.which()? {
            TypeKind::Void(()) => quote!(()),
            TypeKind::Bool(()) => value_or_default!(bool),
            TypeKind::Int8(()) => value_or_default!(int8),
            TypeKind::Uint8(()) => value_or_default!(uint8),
            TypeKind::Int16(()) => value_or_default!(int16),
            TypeKind::Uint16(()) => value_or_default!(uint16),
            TypeKind::Int32(()) => value_or_default!(int32),
            TypeKind::Uint32(()) => value_or_default!(uint32),
            TypeKind::Int64(()) => value_or_default!(int64),
            TypeKind::Uint64(()) => value_or_default!(uint64),
            TypeKind::Float32(()) => value_or_default!(float32),
            TypeKind::Float64(()) => value_or_default!(float64),
            TypeKind::Text(()) => {
                match value.and_then(|v| v.text().field()).map(|v| v.get()) {
                    Some(text) if !text.is_empty() => {
                        let bytes = syn::LitByteStr::new(
                            text.as_bytes_with_nul(),
                            proc_macro2::Span::call_site()
                        );

                        quote!(_p::text::Reader::from_slice(#bytes))
                    }
                    _ => quote!(_p::text::Reader::empty()),
                }
            }
            TypeKind::Data(()) => {
                match value.and_then(|v| v.data().field()).map(|v| v.get()) {
                    Some(data) if !data.is_empty() => {
                        let bytes = syn::LitByteStr::new(
                            data.as_slice(),
                            proc_macro2::Span::call_site()
                        );

                        quote!(_p::data::Reader::from_slice(#bytes))
                    }
                    _ => quote!(_p::data::Reader::empty()),
                }
            }
            TypeKind::List(list) => {
                let element_type = self.resolve_type(scope, &list.element_type().get())?;
                let any = value.and_then(|v| v.list().field())
                    .map(|v| v.ptr().read_as::<AnyList>());
                match any {
                    Some(list) if list.len() != 0 => {
                        let size = list.total_size().context("failed to calculate size of list default")?;
                        assert_eq!(size.caps, 0, "default value contains caps!");
                        assert!(size.words < SegmentOffset::MAX_VALUE as u64, "default value is too large to fit in a single segment!");

                        let size = AllocLen::new(size.words as u32 + 1).unwrap();
                        let mut space = alloc::DynSpace::new(size);
                        let alloc = alloc::Scratch::with_dyn_space(&mut space, alloc::Never);

                        let mut message = recapn::message::Message::new(alloc);
                        let mut builder = message.builder();
                        builder.by_ref().into_root().try_set_any_list(&list, UnwrapErrors).unwrap();
    
                        let result = builder.segments().first();
                        assert_eq!(result.len(), size.get(), "written list value doesn't match size of original");

                        // Remove the root pointer since we're going to create a direct pointer
                        // to the data itself which must be allocated immediately after it.
                        let words = result.as_words().split_first().unwrap().1;
                        let len = list.as_ref().len();
                        let element_size = list.as_ref().element_size();

                        todo!()
                    }
                    _ => quote!(_p::ListReader::empty(_p::ElementSize::size_of::<#element_type>())),
                }
            }
            TypeKind::Enum(info) => {
                let value = value.and_then(|v| v.r#enum().get()).unwrap_or(0);
                let NodeContext {
                    info: Some(NodeInfo::Enum(EnumInfo {
                        enumerants,
                        type_info,
                    })),
                    ..
                } = &self.nodes[&info.type_id()] else { anyhow::bail!("expected enum node") };
                let type_name = type_info.resolve_path(scope);
                let enumerant = &enumerants[value as usize];

                quote!(#type_name::#enumerant)
            }
            TypeKind::Struct(_) => {
                let any = value.and_then(|v| v.r#struct().field())
                    .and_then(|v| v.ptr().try_read_option_as::<AnyStruct>().ok().flatten());
                match any {
                    Some(value) if !value.as_ref().size().is_empty() => {
                        let size = value.total_size().context("failed to calculate size of struct default")?;
                        assert_eq!(size.caps, 0, "default value contains caps!");
                        assert!(size.words < SegmentOffset::MAX_VALUE as u64, "default value is too large to fit in a single segment!");

                        let size = AllocLen::new(size.words as u32 + 1).unwrap();
                        let mut space = alloc::DynSpace::new(size);
                        let alloc = alloc::Scratch::with_dyn_space(&mut space, alloc::Never);

                        let mut message = recapn::message::Message::new(alloc);
                        let mut builder = message.builder();
                        builder.by_ref().into_root().try_set_any_struct(&value, UnwrapErrors).unwrap();
    
                        let result = builder.segments().first();
                        assert_eq!(result.len(), size.get(), "written struct value doesn't match size of original");

                        // Remove the root pointer since we're going to create a direct pointer
                        // to the data itself which must be allocated immediately after it.
                        let words = words_lit(result.as_words().split_first().unwrap().1);
                        let StructSize { data, ptrs } = value.as_ref().size();
                        let size = quote!(_p::StructSize { data: #data, ptrs: #ptrs });

                        quote!(_p::StructReader::slice_unchecked(#words, #size))
                    }
                    _ => quote!(_p::StructReader::empty()),
                }
            }
            TypeKind::Interface(_) => {
                unimplemented!("cannot generate values for Capability fields")
            }
            TypeKind::AnyPointer(kind) => {
                let ptr = value.and_then(|v| v.any_pointer().field())
                    .filter(|p| !p.is_null());
                match kind.which()? {
                    AnyPtrKind::Unconstrained(unconstrained) => match (unconstrained.which()?, ptr) {
                        (ConstraintKind::AnyKind(_), None) => {
                            quote!(_p::ptr::PtrReader::null())
                        }
                        (ConstraintKind::Struct(_), None) => {
                            quote!(_p::ptr::StructReader::empty())
                        }
                        (ConstraintKind::List(_), None) => {
                            quote!(_p::ptr::ListReader::empty())
                        }
                        (ConstraintKind::Capability(_), None) => quote!(()),
                        _ => todo!("generate default values for 'unconstrained' ptr types")
                    },
                    AnyPtrKind::Parameter(_) => todo!("generate default values for generic types"),
                    _ => unreachable!(),
                }
            }
        };

        Ok(expr)
    }

    fn generate_enum(
        &self,
        EnumInfo {
            type_info: TypeInfo { type_ident, .. },
            enumerants,
        }: &EnumInfo,
    ) -> Result<GeneratedEnum> {
        Ok(GeneratedEnum { name: type_ident.clone(), enumerants: enumerants.clone() })
    }

    fn generate_const(&self, node: &ReaderOf<Const>, info: &ConstInfo) -> Result<GeneratedConst> {
        let type_info = node.r#type().get();
        Ok(GeneratedConst {
            ident: info.ident.clone(),
            const_type: self.resolve_type(&info.scope, &type_info)?,
            value: self.generate_value(&info.scope, &type_info, node.value().get_option().as_ref())?,
        })
    }
}

fn words_lit(words: &[Word]) -> TokenStream {
    let words = words.iter().map(|Word([b0, b1, b2, b3, b4, b5, b6, b7])| {
        quote!(Word([#b0, #b1, #b2, #b3, #b4, #b5, #b6, #b7]))
    });

    quote!(&[#(#words)*])
}