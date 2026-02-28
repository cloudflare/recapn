use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::ptr::NonNull;
use std::str::Utf8Error;

use heck::{AsPascalCase, AsShoutySnakeCase, AsSnakeCase};
use proc_macro2::TokenStream;
use quote::quote;
use recapn::alloc::{self, AllocLen, Segment, SegmentOffset, Word};
use recapn::any::{AnyList, AnyStruct};
use recapn::ptr::{ElementSize, StructSize, UnwrapErrors};
use recapn::{any, data, text, ReaderOf};
use syn::punctuated::Punctuated;
use syn::PathSegment;

use crate::quotes::{
    FieldDescriptor, GeneratedConst, GeneratedEnum, GeneratedField, GeneratedFile, GeneratedInterface, GeneratedItem, GeneratedPipelineOp, GeneratedRootFile, GeneratedStruct, GeneratedVariant, GeneratedWhich, PipelineType
};
use crate::schema::{
    schema_capnp, AnnotationSchema, ConstSchema, EnumSchema, FieldType, FileSchema, Id,
    InterfaceSchema, NamedNestedItem, NestedItem, Schema, SchemaError, SchemaLoader, StructSchema,
    Type, TypeKind,
};
use crate::{Error, IncludeRpc, Result};

pub mod ident;

use self::ident::{IdentifierSet, Scope, ScopedIdentifierSet};

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

/// Resolve a path to a type from the given reference scope.
///
/// For example in the case where we want to refer to A from B, B is the ref scope we're
/// resolving from.
fn resolve_path(
    type_ident: &syn::Ident,
    type_scope: &TypeScope,
    ref_scope: &TypeScope,
) -> syn::Path {
    if ref_scope == type_scope {
        // We're referencing this type from the same scope, so we can just use the type identifier.
        return syn::Path::from(type_ident.clone());
    }

    let mut segments: Punctuated<PathSegment, _> = Punctuated::new();
    let mut mod_path = type_scope.types.as_slice();
    if ref_scope.file == type_scope.file {
        // First, check if we're attempting to refer to a type *in* the parent scope.
        if let Some((_, ref_parent_types)) = ref_scope.types.split_last() {
            if mod_path == ref_parent_types {
                // It's the parent scope! So we can use `super` directly instead of `__file`
                return syn::parse_quote!(super::#type_ident);
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
        segments.push(PathSegment::from(type_scope.file.mod_ident.clone()));
    }

    segments.extend(
        mod_path
            .iter()
            .map(|s| PathSegment::from(s.mod_ident.clone())),
    );
    segments.push(PathSegment::from(type_ident.clone()));

    syn::Path {
        leading_colon: None,
        segments,
    }
}

#[derive(Debug)]
struct FileInfo {
    pub mod_ident: syn::Ident,
    pub path: String,
}

#[derive(Debug)]
struct StructInfo {
    pub scope: TypeScope,
    pub struct_ident: syn::Ident,
    pub mod_ident: syn::Ident,
}

#[derive(Debug)]
struct EnumInfo {
    pub scope: TypeScope,
    pub enum_ident: syn::Ident,
    pub enumerants: Vec<syn::Ident>,
}

#[derive(Debug)]
struct ConstInfo {
    pub ident: syn::Ident,
    pub scope: TypeScope,
}

#[derive(Debug)]
struct InterfaceInfo {
    pub scope: TypeScope,
    pub client_ident: syn::Ident,
    pub server_ident: syn::Ident,
    pub dispatch_ident: syn::Ident,
    pub mod_ident: syn::Ident,
}

#[derive(Debug)]
struct AnnotationInfo {}

#[derive(Debug)]
enum NodeInfo {
    File(FileInfo),
    Struct(StructInfo),
    Enum(EnumInfo),
    Const(ConstInfo),
    Interface(InterfaceInfo),
    Annotation(AnnotationInfo),
}

impl NodeInfo {
    pub fn unwrap_file(&self) -> &FileInfo {
        if let Self::File(info) = self {
            info
        } else {
            panic!("expected file node info")
        }
    }

    pub fn unwrap_struct(&self) -> &StructInfo {
        if let Self::Struct(info) = self {
            info
        } else {
            panic!("expected struct node info")
        }
    }

    pub fn unwrap_enum(&self) -> &EnumInfo {
        if let Self::Enum(info) = self {
            info
        } else {
            panic!("expected enum node info")
        }
    }

    pub fn unwrap_const(&self) -> &ConstInfo {
        if let Self::Const(info) = self {
            info
        } else {
            panic!("expected const node info")
        }
    }

    pub fn unwrap_interface(&self) -> &InterfaceInfo {
        if let Self::Interface(info) = self {
            info
        } else {
            panic!("expeceted interface node info")
        }
    }
}

type NodeInfoMap = HashMap<u64, NodeInfo>;

#[derive(Debug)]
pub enum NameContext {
    Node(Id),
    Field { type_id: Id, index: u32 },
    Enumerant { type_id: Id, index: u32 },
    Method { type_id: Id, index: u32 },
}

impl fmt::Display for NameContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Node(id) => write!(f, "node {:0<#16x}", id),
            Self::Field { type_id, index } => {
                write!(f, "field @{} in struct @{:0<#16x}", index, type_id)
            }
            Self::Enumerant { type_id, index } => {
                write!(f, "enumerant @{} in enum @{:0<#16x}", index, type_id)
            }
            Self::Method { type_id, index } => {
                write!(f, "method @{} in interface @{:0<#16x}", index, type_id)
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GeneratorError {
    #[error("name for {context} is not valid UTF8: {error}")]
    InvalidName {
        context: NameContext,
        error: Utf8Error,
    },
    #[error("item has empty name")]
    EmptyName,
}

struct NodeValidation {
    uses_rpc: bool,
}

fn validate_file(
    schema: &FileSchema<'_>,
    nodes: &mut NodeInfoMap,
    identifiers: &mut ScopedIdentifierSet<ModScope>,
) -> Result<NodeValidation> {
    let id = schema.node.id();
    let Entry::Vacant(entry) = nodes.entry(id) else {
        return Err(SchemaError::DuplicateNode(id))?;
    };

    let path =
        schema
            .node
            .display_name()
            .get()
            .as_str()
            .map_err(|error| GeneratorError::InvalidName {
                context: NameContext::Node(id),
                error,
            })?;
    let mod_ident = identifiers.make_unique(None, path)?;
    let file_path = format!("{path}.rs");
    entry.insert(NodeInfo::File(FileInfo {
        mod_ident: mod_ident.clone(),
        path: file_path,
    }));

    let mut scope = Scope::file(ModScope { id, mod_ident });

    let mut file_uses_rpc = false;
    for nested in schema.nested_items() {
        let nested = match nested {
            Ok(n) => n,
            // Unreferenced nodes from files not requested can be missing, so we just continue.
            // If it turns out we needed it, an error will come up later.
            Err(Error::Schema(SchemaError::MissingNode(_))) => continue,
            res @ Err(_) => res?,
        };
        let NodeValidation { uses_rpc } = validate_item(nested, nodes, identifiers, &mut scope)?;
        file_uses_rpc |= uses_rpc;
    }

    Ok(NodeValidation { uses_rpc: file_uses_rpc })
}

fn validate_item(
    node: NamedNestedItem<'_>,
    nodes: &mut NodeInfoMap,
    identifiers: &mut ScopedIdentifierSet<ModScope>,
    scope: &mut TypeScope,
) -> Result<NodeValidation> {
    let name = node
        .name
        .as_str()
        .map_err(|error| GeneratorError::InvalidName {
            context: NameContext::Node(node.id),
            error,
        })?;
    match &node.item {
        NestedItem::Struct(schema) => validate_struct(schema, name, nodes, identifiers, scope),
        NestedItem::Enum(schema) => validate_enum(schema, name, nodes, identifiers, scope),
        NestedItem::Interface(schema) => {
            validate_interface(schema, name, nodes, identifiers, scope)
        }
        NestedItem::Const(schema) => validate_const(schema, name, nodes, identifiers, scope),
        NestedItem::Annotation(schema) => {
            validate_annotation(schema, name, nodes, identifiers, scope)
        }
    }
}

fn validate_struct(
    schema: &StructSchema<'_>,
    name: &str,
    nodes: &mut NodeInfoMap,
    identifiers: &mut ScopedIdentifierSet<ModScope>,
    scope: &mut TypeScope,
) -> Result<NodeValidation> {
    let id = schema.node.id();
    let Entry::Vacant(entry) = nodes.entry(id) else {
        return Err(SchemaError::DuplicateNode(id))?;
    };

    let struct_ident = identifiers.make_unique(Some(scope.clone()), AsPascalCase(name))?;
    let mod_ident = identifiers.make_unique(Some(scope.clone()), AsSnakeCase(name))?;

    let node_info = StructInfo {
        scope: scope.clone(),
        struct_ident,
        mod_ident: mod_ident.clone(),
    };

    entry.insert(NodeInfo::Struct(node_info));

    scope.push(ModScope { id, mod_ident });

    if schema.info.discriminant_count() != 0 {
        // Insert Which into the identifier set so if any conflicts appear they'll
        // be made properly unique
        let _ = identifiers.make_unique(Some(scope.clone()), "Which")?;
    }

    // And these reserved names.
    let _ = identifiers.make_unique(Some(scope.clone()), "Reader")?;
    let _ = identifiers.make_unique(Some(scope.clone()), "Builder")?;
    let _ = identifiers.make_unique(Some(scope.clone()), "Pipeline")?;

    for nested in schema.nested_items() {
        validate_item(nested?, nodes, identifiers, scope)?;
    }

    let mut uses_rpc = false;

    for (idx, field) in schema.fields().enumerate() {
        match field.field_type()? {
            FieldType::Slot { field_type, .. } => match field_type.kind {
                TypeKind::Interface { .. } |
                TypeKind::AnyCapability => {
                    uses_rpc = true;
                },
                _ => {
                    // Assume that "any" types other than any capability are not RPC.
                }
            },
            // For some reason, groups are not considered nested nodes of structs. So we
            // have to iter over all fields and find all the groups contained
            FieldType::Group(schema) => {
                let name = field
                    .name()
                    .as_str()
                    .map_err(|error| GeneratorError::InvalidName {
                        context: NameContext::Field {
                            type_id: id,
                            index: idx as u32,
                        },
                        error,
                    })?;
                let validation = validate_struct(&schema, name, nodes, identifiers, scope)?;
                uses_rpc |= validation.uses_rpc;
            },
        }
    }

    scope.pop();
    Ok(NodeValidation { uses_rpc })
}

fn validate_enum(
    schema: &EnumSchema<'_>,
    name: &str,
    nodes: &mut NodeInfoMap,
    identifiers: &mut ScopedIdentifierSet<ModScope>,
    scope: &mut TypeScope,
) -> Result<NodeValidation> {
    let id = schema.node.id();
    let Entry::Vacant(entry) = nodes.entry(id) else {
        return Err(SchemaError::DuplicateNode(id))?;
    };

    let enum_ident = identifiers.make_unique(Some(scope.clone()), AsPascalCase(name))?;

    let enumerants = {
        let mut idents = IdentifierSet::new();
        schema
            .enumerants()
            .enumerate()
            .map(|(idx, name)| {
                let name = name.as_str().map_err(|error| GeneratorError::InvalidName {
                    context: NameContext::Enumerant {
                        type_id: id,
                        index: idx as u32,
                    },
                    error,
                })?;
                let ident = idents.make_unique(AsPascalCase(name))?;
                Ok(ident)
            })
            .collect::<Result<_>>()?
    };

    entry.insert(NodeInfo::Enum(EnumInfo {
        scope: scope.clone(),
        enum_ident,
        enumerants,
    }));

    Ok(NodeValidation { uses_rpc: false })
}

fn validate_interface(
    schema: &InterfaceSchema<'_>,
    name: &str,
    nodes: &mut NodeInfoMap,
    identifiers: &mut ScopedIdentifierSet<ModScope>,
    scope: &mut TypeScope,
) -> Result<NodeValidation> {
    let id = schema.node.id();
    let Entry::Vacant(entry) = nodes.entry(id) else {
        return Err(SchemaError::DuplicateNode(id))?;
    };

    let client_ident = identifiers.make_unique(Some(scope.clone()), AsPascalCase(name))?;
    let server_ident = identifiers.make_unique(Some(scope.clone()), AsPascalCase(format!("{name}Server")))?;
    let dispatch_ident = identifiers.make_unique(Some(scope.clone()), AsPascalCase(format!("{name}Dispatcher")))?;
    let mod_ident = identifiers.make_unique(Some(scope.clone()), AsSnakeCase(name))?;

    let node_info = InterfaceInfo {
        scope: scope.clone(),
        client_ident,
        server_ident,
        dispatch_ident,
        mod_ident: mod_ident.clone(),
    };

    entry.insert(NodeInfo::Interface(node_info));

    scope.push(ModScope { id, mod_ident });

    for nested in schema.nested_items() {
        validate_item(nested?, nodes, identifiers, scope)?;
    }

    // Again, autogenerated method structs are not nested members of interfaces, so
    // we have to generate them.
    for (idx, method) in schema.info.methods().into_iter().enumerate() {
        let method_name = method
            .name()
            .as_str()
            .map_err(|error| GeneratorError::InvalidName {
                context: NameContext::Field {
                    type_id: id,
                    index: idx as u32,
                },
                error,
            })?;
        let param_schema = schema
            .loader()
            .try_schema::<StructSchema<'_>>(method.param_struct_type())?;
        if param_schema.node.scope_id() == 0 {
            let param_name = format!("{method_name}Params");
            validate_struct(&param_schema, &param_name, nodes, identifiers, scope)?;
        }
        let result_schema = schema
            .loader()
            .try_schema::<StructSchema<'_>>(method.result_struct_type())?;
        if result_schema.node.scope_id() == 0 {
            let result_name = format!("{method_name}Results");
            validate_struct(&result_schema, &result_name, nodes, identifiers, scope)?;
        }
    }

    scope.pop();

    Ok(NodeValidation { uses_rpc: true })
}

fn validate_const(
    schema: &ConstSchema<'_>,
    name: &str,
    nodes: &mut NodeInfoMap,
    identifiers: &mut ScopedIdentifierSet<ModScope>,
    scope: &mut TypeScope,
) -> Result<NodeValidation> {
    let id = schema.node.id();
    let Entry::Vacant(entry) = nodes.entry(id) else {
        return Err(SchemaError::DuplicateNode(id))?;
    };

    entry.insert(NodeInfo::Const(ConstInfo {
        scope: scope.clone(),
        ident: identifiers.make_unique(Some(scope.clone()), AsShoutySnakeCase(name))?,
    }));

    Ok(NodeValidation { uses_rpc: false })
}

fn validate_annotation(
    schema: &AnnotationSchema<'_>,
    _: &str,
    nodes: &mut NodeInfoMap,
    _: &mut ScopedIdentifierSet<ModScope>,
    _: &mut TypeScope,
) -> Result<NodeValidation> {
    let id = schema.node.id();
    let Entry::Vacant(entry) = nodes.entry(id) else {
        return Err(SchemaError::DuplicateNode(id))?;
    };

    entry.insert(NodeInfo::Annotation(AnnotationInfo {}));

    // do nothing for now
    Ok(NodeValidation { uses_rpc: false })
}

macro_rules! quote_none {
    () => {
        syn::parse_quote!(::core::option::Option::None)
    };
}

/// Context related to generating a single file
struct FileContext {
    required_imports: BTreeSet<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TypeContext {
    Field,
    Const,
}

pub struct GeneratorContext {
    loader: SchemaLoader,
    info: NodeInfoMap,
    include_rpc: bool,
}

impl GeneratorContext {
    pub fn new(
        request: &ReaderOf<'_, schema_capnp::CodeGeneratorRequest>,
        include_rpc: IncludeRpc,
    ) -> Result<Self> {
        let loader = {
            let mut loader = SchemaLoader::new();
            for node in request.nodes() {
                loader.load(&node)?;
            }
            loader
        };
        let mut info = NodeInfoMap::new();
        let mut identifiers = ScopedIdentifierSet::new();

        let mut files_use_rpc = false;
        for file in loader.files() {
            let validation = validate_file(&file, &mut info, &mut identifiers)?;
            files_use_rpc |= validation.uses_rpc;
        }

        let include_rpc = match include_rpc {
            IncludeRpc::Automatic => files_use_rpc,
            IncludeRpc::No => false,
            IncludeRpc::Yes => true,
        };

        Ok(Self { loader, info, include_rpc })
    }

    fn find_info(&self, id: Id) -> Result<&NodeInfo> {
        self.info
            .get(&id)
            .ok_or(SchemaError::MissingNode(id).into())
    }

    pub fn generate_file(&self, id: u64) -> Result<(GeneratedFile, GeneratedRootFile)> {
        let schema: FileSchema<'_> = self.loader.try_schema(id)?;
        let info = self.find_info(id)?.unwrap_file();

        let mut file = GeneratedFile {
            items: Vec::new(),
            ident: info.mod_ident.clone(),
        };

        let mut ctx = FileContext {
            required_imports: BTreeSet::new(),
        };

        for nested in schema.nested_items() {
            let nested = nested?;
            if let Some(item) = self.generate_item(nested.id, nested.item, &mut ctx)? {
                file.items.push(item);
            }
        }

        let mut root_mod = GeneratedRootFile {
            ident: info.mod_ident.clone(),
            imports: Vec::with_capacity(ctx.required_imports.len()),
            path: info.path.clone(),
        };
        for import in ctx.required_imports {
            let info = self.find_info(import)?.unwrap_file();
            root_mod.imports.push(info.mod_ident.clone());
        }

        Ok((file, root_mod))
    }

    fn generate_item(
        &self,
        id: u64,
        item: NestedItem<'_>,
        ctx: &mut FileContext,
    ) -> Result<Option<GeneratedItem>> {
        let info = self.find_info(id)?;
        match item {
            NestedItem::Struct(schema) => {
                let generated = self.generate_struct(&schema, info.unwrap_struct(), ctx)?;
                Ok(Some(GeneratedItem::Struct(generated)))
            }
            NestedItem::Enum(schema) => {
                let generated = self.generate_enum(&schema, info.unwrap_enum(), ctx)?;
                Ok(Some(GeneratedItem::Enum(generated)))
            }
            NestedItem::Const(schema) => {
                let generated = self.generate_const(&schema, info.unwrap_const(), ctx)?;
                Ok(Some(GeneratedItem::Const(generated)))
            }
            NestedItem::Interface(schema) if self.include_rpc => {
                let generated = self.generate_interface(&schema, info.unwrap_interface(), ctx)?;
                Ok(Some(GeneratedItem::Interface(generated)))
            },
            NestedItem::Interface(_) |
            NestedItem::Annotation(_) => Ok(None),
        }
    }

    fn generate_struct(
        &self,
        schema: &StructSchema<'_>,
        info: &StructInfo,
        ctx: &mut FileContext,
    ) -> Result<GeneratedStruct> {
        let (fields, variants) = schema
            .fields()
            .enumerate() // enumerate the fields so we can report name errors better
            .partition::<Vec<_>, _>(|(_, field)| field.discriminant().is_none());

        assert_eq!(schema.info.discriminant_count() as usize, variants.len());

        let size = (!schema.is_group()).then(|| schema.struct_size());

        let mut generated = GeneratedStruct {
            id: schema.node.id(),
            ident: info.struct_ident.clone(),
            mod_ident: info.mod_ident.clone(),
            type_params: Vec::new(),
            size,
            fields: Vec::new(),
            pipeline_fields: Vec::new(),
            which: None,
            nested_items: Vec::new(),
        };

        let mut discriminant_idents = IdentifierSet::new();
        let mut descriptor_idents = IdentifierSet::new();
        let mut accessor_idents = IdentifierSet::new();

        generated.fields.reserve(fields.len());
        for (idx, field) in fields {
            let name = field
                .name()
                .as_str()
                .map_err(|error| GeneratorError::InvalidName {
                    context: NameContext::Field {
                        type_id: schema.node.id(),
                        index: idx as u32,
                    },
                    error,
                })?;
            let field_type = field.field_type()?;
            let generated_field = self.generate_field(
                &field_type,
                name,
                &info.scope,
                &info.struct_ident,
                &mut descriptor_idents,
                &mut accessor_idents,
                ctx,
            )?;
            if self.include_rpc {
                generated.pipeline_fields.extend(self.generate_pipeline_op(
                    &field_type,
                    &info.scope,
                    generated_field.accessor_ident.clone(),
                )?);
            }
            generated.fields.push(generated_field);
        }

        if !variants.is_empty() {
            let which = generated.which.insert(GeneratedWhich {
                tag_slot: schema.info.discriminant_offset(),
                fields: Vec::new(),
                type_params: Vec::new(),
            });
            // Create a scope used for resolving types from within the struct's mod scope.
            let struct_mod_scope = {
                let mut struct_scope = info.scope.clone();
                struct_scope.push(ModScope {
                    id: schema.node.id(),
                    mod_ident: info.mod_ident.clone(),
                });
                struct_scope
            };
            which.fields.reserve(variants.len());
            for (idx, field) in variants {
                let name = field
                    .name()
                    .as_str()
                    .map_err(|error| GeneratorError::InvalidName {
                        context: NameContext::Field {
                            type_id: schema.node.id(),
                            index: idx as u32,
                        },
                        error,
                    })?;
                let discriminant_ident = discriminant_idents.make_unique(AsPascalCase(name))?;

                let field_type = field.field_type()?;
                let generated_field = self.generate_field(
                    &field_type,
                    name,
                    &info.scope,
                    &info.struct_ident,
                    &mut descriptor_idents,
                    &mut accessor_idents,
                    ctx,
                )?;
                which.fields.push(GeneratedVariant {
                    discriminant_ident,
                    discriminant_field_type: self.resolve_field_type(
                        &struct_mod_scope,
                        &field_type,
                        ctx,
                    )?,
                    case: field.info.discriminant_value(),
                    field: generated_field,
                })
            }
        }

        for nested in schema.nested_items() {
            let nested = nested?;
            if let Some(item) = self.generate_item(nested.id, nested.item, ctx)? {
                generated.nested_items.push(item);
            }
        }

        for field in schema.fields() {
            if let FieldType::Group(group) = field.field_type()? {
                let info = self.find_info(group.node.id())?.unwrap_struct();
                let item = self.generate_struct(&group, info, ctx)?;
                generated.nested_items.push(GeneratedItem::Struct(item));
            }
        }

        Ok(generated)
    }

    fn generate_field(
        &self,
        field_type: &FieldType<'_>,
        name: &str,
        scope: &TypeScope,
        struct_ident: &syn::Ident,
        descriptor_idents: &mut IdentifierSet,
        accessor_idents: &mut IdentifierSet,
        ctx: &mut FileContext,
    ) -> Result<GeneratedField> {
        let accessor_ident = accessor_idents.make_unique(AsSnakeCase(name))?;
        let descriptor_ident = descriptor_idents.make_unique(AsShoutySnakeCase(name))?;

        let has_own_accessor;
        let descriptor;
        match field_type {
            FieldType::Group(..) => {
                has_own_accessor = true;
                descriptor = None;
            }
            FieldType::Slot {
                field_type,
                offset,
                default_value,
            } => {
                has_own_accessor = field_type.is_ptr_field();
                let void_field = matches!(
                    field_type,
                    Type {
                        list_depth: 0,
                        kind: TypeKind::Void
                    },
                );
                descriptor = if void_field {
                    None
                } else {
                    let default_value = match default_value {
                        Some(default) => {
                            self.generate_field_default_value(scope, field_type, default)
                        }
                        None => self.generate_field_default_for_type(scope, field_type),
                    }?;
                    Some(FieldDescriptor {
                        slot: *offset,
                        default: default_value,
                    })
                };
            }
        };
        let syn_type = self.resolve_field_type(scope, field_type, ctx)?;
        let own_accessor_ident = if has_own_accessor {
            Some(accessor_idents.make_unique(AsSnakeCase(format!("into_{name}")))?)
        } else {
            None
        };

        Ok(GeneratedField {
            type_name: struct_ident.clone(),
            field_type: syn_type,
            own_accessor_ident,
            accessor_ident,
            descriptor_ident,
            descriptor,
        })
    }

    fn generate_pipeline_op(
        &self,
        field_type: &FieldType<'_>,
        scope: &TypeScope,
        name: syn::Ident,
    ) -> Result<Option<GeneratedPipelineOp>> {
        let return_type;
        let pipelined;
        match field_type {
            FieldType::Slot { offset, field_type, .. } => {
                if field_type.is_list() || field_type.is_data_field() {
                    return Ok(None)
                }

                let slot = *offset as u16;

                match field_type.kind {
                    TypeKind::AnyList | TypeKind::Data | TypeKind::Text => return Ok(None),
                    TypeKind::AnyCapability | TypeKind::Interface { .. } => {
                        pipelined = PipelineType::Capability(slot);
                    }
                    TypeKind::Struct { .. } |
                    TypeKind::AnyPointer |
                    TypeKind::AnyStruct |
                    TypeKind::ScopeBound { .. } |
                    TypeKind::ImplicitMethodParameter { .. } => {
                        pipelined = PipelineType::Field(slot);
                    }
                    _ => unreachable!(),
                }

                match &field_type.kind {
                    TypeKind::Interface { schema, .. } => {
                        return_type = syn::TypePath {
                            qself: None, path: self.resolve_interface_type(scope, schema)?
                        }.into();
                    }
                    TypeKind::AnyCapability => {
                        return_type = syn::parse_quote!(::recapn_rpc::client::Client);
                    },
                    TypeKind::Struct { schema, .. } => {
                        let path = self.resolve_struct_type(scope, schema)?;
                        return_type = syn::parse_quote!(::recapn::rpc::PipelineOf<#path, P>);
                    }
                    TypeKind::AnyPointer |
                    TypeKind::AnyStruct |
                    TypeKind::ScopeBound { .. } |
                    TypeKind::ImplicitMethodParameter { .. } => {
                        return_type = syn::parse_quote!(::recapn::rpc::PipelineOf<::recapn::any::AnyPtr, P>);
                    }
                    _ => unreachable!(),
                }
            },
            FieldType::Group(group) => {
                let path = self.resolve_struct_type(scope, group)?;
                return_type = syn::parse_quote!(::recapn::rpc::PipelineOf<#path, P>);
                pipelined = PipelineType::Group;
            },
        }

        Ok(Some(GeneratedPipelineOp { field_name: name, return_type, pipelined }))
    }

    fn resolve_interface_type(
        &self,
        scope: &TypeScope,
        schema: &InterfaceSchema<'_>,
    ) -> Result<syn::Path> {
        let interface_type = self.find_info(schema.node.id())?.unwrap_interface();
        let path = resolve_path(&interface_type.client_ident, &interface_type.scope, scope);
        Ok(path)
    }

    fn resolve_struct_type(
        &self,
        scope: &TypeScope,
        schema: &StructSchema<'_>,
    ) -> Result<syn::Path> {
        let struct_type = self.find_info(schema.node.id())?.unwrap_struct();
        let path = resolve_path(&struct_type.struct_ident, &struct_type.scope, scope);
        Ok(path)
    }

    fn resolve_field_type(
        &self,
        scope: &TypeScope,
        field_type: &FieldType<'_>,
        ctx: &mut FileContext,
    ) -> Result<Box<syn::Type>> {
        Ok(match field_type {
            FieldType::Group(group) => {
                let path = self.resolve_struct_type(scope, group)?;
                Box::new(syn::parse_quote!(_p::Group<#path>))
            }
            FieldType::Slot { field_type, .. } => {
                self.resolve_type(scope, field_type, TypeContext::Field, ctx)?
            }
        })
    }

    fn resolve_type(
        &self,
        scope: &TypeScope,
        info: &Type<'_>,
        ty_ctx: TypeContext,
        ctx: &mut FileContext,
    ) -> Result<Box<syn::Type>> {
        let mut rust_ty = match &info.kind {
            TypeKind::Void => syn::parse_quote!(()),
            TypeKind::Bool => syn::parse_quote!(bool),
            TypeKind::Int8 => syn::parse_quote!(i8),
            TypeKind::Int16 => syn::parse_quote!(i16),
            TypeKind::Int32 => syn::parse_quote!(i32),
            TypeKind::Int64 => syn::parse_quote!(i64),
            TypeKind::Uint8 => syn::parse_quote!(u8),
            TypeKind::Uint16 => syn::parse_quote!(u16),
            TypeKind::Uint32 => syn::parse_quote!(u32),
            TypeKind::Uint64 => syn::parse_quote!(u64),
            TypeKind::Float32 => syn::parse_quote!(f32),
            TypeKind::Float64 => syn::parse_quote!(f64),
            TypeKind::Enum { schema, .. } => {
                let type_info = &self.find_info(schema.node.id())?.unwrap_enum();
                if type_info.scope.file != scope.file {
                    ctx.required_imports.insert(type_info.scope.file.id);
                }

                let path = resolve_path(&type_info.enum_ident, &type_info.scope, scope);
                if ty_ctx == TypeContext::Const && !info.is_list() {
                    syn::parse_quote!(#path)
                } else {
                    syn::parse_quote!(_p::Enum<#path>)
                }
            }
            TypeKind::Data => syn::parse_quote!(_p::Data),
            TypeKind::Text => syn::parse_quote!(_p::Text),
            TypeKind::Struct { schema, .. } => {
                let type_info = &self.find_info(schema.node.id())?.unwrap_struct();
                if type_info.scope.file != scope.file {
                    ctx.required_imports.insert(type_info.scope.file.id);
                }

                let path = resolve_path(&type_info.struct_ident, &type_info.scope, scope);
                syn::parse_quote!(_p::Struct<#path>)
            }
            TypeKind::Interface { schema, .. } => {
                let type_info = &self.find_info(schema.node.id())?.unwrap_interface();
                if type_info.scope.file != scope.file {
                    ctx.required_imports.insert(type_info.scope.file.id);
                }

                let path = resolve_path(&type_info.client_ident, &type_info.scope, scope);
                syn::parse_quote!(_p::Capability<#path>)
            }
            TypeKind::AnyPointer => syn::parse_quote!(_p::AnyPtr),
            TypeKind::AnyStruct => syn::parse_quote!(_p::AnyStruct),
            TypeKind::AnyList => syn::parse_quote!(_p::AnyList),
            TypeKind::AnyCapability => syn::parse_quote!(_p::Capability<::recapn_rpc::client::Client>),
            TypeKind::ScopeBound { .. } => syn::parse_quote!(_p::AnyPtr), // TODO
            TypeKind::ImplicitMethodParameter { .. } => syn::parse_quote!(_p::AnyPtr), // TODO
        };
        for _ in 0..info.list_depth {
            rust_ty = syn::parse_quote!(_p::List<#rust_ty>);
        }
        if ty_ctx == TypeContext::Const && info.is_ptr_field() {
            rust_ty = syn::parse_quote!(_p::ty::ConstPtr<#rust_ty>)
        }
        Ok(rust_ty)
    }

    fn generate_field_default_for_type(
        &self,
        scope: &TypeScope,
        info: &Type<'_>,
    ) -> Result<Box<syn::Expr>> {
        if info.list_depth != 0 {
            return Ok(quote_none!());
        }
        use TypeKind::*;
        let tokens = match &info.kind {
            Void => syn::parse_quote!(()),
            Bool => syn::parse_quote!(false),
            Int8 => syn::parse_quote!(0i8),
            Int16 => syn::parse_quote!(0i16),
            Int32 => syn::parse_quote!(0i32),
            Int64 => syn::parse_quote!(0i64),
            Uint8 => syn::parse_quote!(0u8),
            Uint16 => syn::parse_quote!(0u16),
            Uint32 => syn::parse_quote!(0u32),
            Uint64 => syn::parse_quote!(0u64),
            Float32 => syn::parse_quote!(0.0f32),
            Float64 => syn::parse_quote!(0.0f64),
            Enum { schema, .. } => {
                let info = self.find_info(schema.node.id())?.unwrap_enum();
                let type_name = resolve_path(&info.enum_ident, &info.scope, scope);
                let enumerant = &info.enumerants[0];

                syn::parse_quote!(#type_name::#enumerant)
            }
            Data
            | Text
            | Struct { .. }
            | Interface { .. }
            | AnyPointer
            | AnyStruct
            | AnyList
            | AnyCapability
            | ScopeBound { .. }
            | ImplicitMethodParameter { .. } => quote_none!(),
        };

        Ok(tokens)
    }

    fn generate_field_default_value(
        &self,
        scope: &TypeScope,
        type_info: &Type<'_>,
        value: &ReaderOf<'_, schema_capnp::Value>,
    ) -> Result<Box<syn::Expr>> {
        if type_info.is_list() {
            let value = value
                .list()
                .field()
                .and_then(|v| v.ptr().read_option_as::<AnyList>())
                .filter(|l| !l.is_empty());
            return Ok(if let Some(value) = value {
                let value_element_size = value.element_size();

                // Now check to make sure we're making a valid pointer for this element type. Since we're
                // going to make an unchecked pointer into the data we have to do this check beforehand.
                // capnpc should give us the same element size as what the type is, but this acts as a
                // simple sanity check.
                let expected_element_size = if type_info.is_list_of_lists() {
                    ElementSize::Pointer
                } else {
                    use TypeKind::*;
                    match &type_info.kind {
                        Void => ElementSize::Void,
                        Bool => ElementSize::Bit,
                        Int8 | Uint8 => ElementSize::Byte,
                        Int16 | Uint16 | Enum { .. } => ElementSize::TwoBytes,
                        Int32 | Uint32 | Float32 => ElementSize::FourBytes,
                        Int64 | Uint64 | Float64 => ElementSize::EightBytes,
                        Struct { schema, .. } => ElementSize::InlineComposite(schema.struct_size()),
                        AnyStruct => {
                            assert!(
                                value_element_size.is_inline_composite(),
                                "expected inline composite elements for list of any struct"
                            );
                            value_element_size
                        }
                        _ => ElementSize::Pointer,
                    }
                };

                assert_eq!(value_element_size, expected_element_size);

                let expr = generate_untyped_list_value(&value);
                syn::parse_quote!(::core::option::Option::Some(#expr))
            } else {
                quote_none!()
            });
        }

        if type_info.is_data_field() {
            return self.generate_data_value(scope, &type_info.kind, value);
        }

        // Now we just need to handle the other pointer field types!
        let expr = match &type_info.kind {
            TypeKind::Text => value
                .text()
                .field()
                .map(|v| v.get())
                .filter(|t| !t.is_empty())
                .map_or_else(
                    || quote_none!(),
                    |text| {
                        let expr = text_to_expr(text);
                        syn::parse_quote!(::core::option::Option::Some(#expr))
                    },
                ),
            TypeKind::Data => value
                .data()
                .field()
                .map(|v| v.get())
                .filter(|d| !d.is_empty())
                .map_or_else(
                    || quote_none!(),
                    |data| {
                        let expr = data_to_expr(data);
                        syn::parse_quote!(::core::option::Option::Some(#expr))
                    },
                ),
            TypeKind::Struct { schema, .. } => value
                .r#struct()
                .field()
                .and_then(|f| f.ptr().read_option_as::<AnyStruct>())
                .filter(|s| !s.as_ref().size().is_empty())
                .map_or_else(
                    || quote_none!(),
                    |s| {
                        assert_eq!(schema.struct_size(), s.as_ref().size());

                        let expr = generate_untyped_struct_value(&s);
                        syn::parse_quote!(::core::option::Option::Some(#expr))
                    },
                ),
            TypeKind::AnyPointer => value
                .any_pointer()
                .field()
                .map(|f| f.ptr())
                .filter(|p| p.is_null())
                .map_or_else(
                    || quote_none!(),
                    |p| {
                        let expr = generate_untyped_ptr_value(&p);
                        syn::parse_quote!(::core::option::Option::Some(#expr))
                    },
                ),
            TypeKind::AnyStruct => value
                .any_pointer()
                .field()
                .and_then(|p| p.ptr().read_option_as::<AnyStruct>())
                .filter(|s| !s.as_ref().size().is_empty())
                .map_or_else(
                    || quote_none!(),
                    |s| {
                        let expr = generate_untyped_struct_value(&s);
                        syn::parse_quote!(::core::option::Option::Some(#expr))
                    },
                ),
            TypeKind::AnyList => value
                .any_pointer()
                .field()
                .and_then(|p| p.ptr().read_option_as::<AnyList>())
                .filter(|s| !s.is_empty())
                .map_or_else(
                    || quote_none!(),
                    |l| {
                        let expr = generate_untyped_list_value(&l);
                        syn::parse_quote!(::core::option::Option::Some(#expr))
                    },
                ),
            TypeKind::Interface { .. }
            | TypeKind::AnyCapability
            | TypeKind::ScopeBound { .. }
            | TypeKind::ImplicitMethodParameter { .. } => quote_none!(),
            _ => unimplemented!(),
        };

        Ok(expr)
    }

    fn generate_data_value(
        &self,
        scope: &TypeScope,
        type_info: &TypeKind<'_>,
        value: &ReaderOf<'_, schema_capnp::Value>,
    ) -> Result<Box<syn::Expr>> {
        macro_rules! value_or_default {
            ($field:ident) => {{
                let value = value.$field().get().unwrap_or_default();
                syn::parse_quote!(#value)
            }};
        }

        let expr = match type_info {
            TypeKind::Void => syn::parse_quote!(()),
            TypeKind::Bool => value_or_default!(bool),
            TypeKind::Int8 => value_or_default!(int8),
            TypeKind::Uint8 => value_or_default!(uint8),
            TypeKind::Int16 => value_or_default!(int16),
            TypeKind::Uint16 => value_or_default!(uint16),
            TypeKind::Int32 => value_or_default!(int32),
            TypeKind::Uint32 => value_or_default!(uint32),
            TypeKind::Int64 => value_or_default!(int64),
            TypeKind::Uint64 => value_or_default!(uint64),
            TypeKind::Float32 => value_or_default!(float32),
            TypeKind::Float64 => value_or_default!(float64),
            TypeKind::Enum { schema, .. } => {
                let value = value.r#enum().get().unwrap_or(0);
                let info = self.find_info(schema.node.id())?.unwrap_enum();

                let type_path = resolve_path(&info.enum_ident, &info.scope, scope);
                let enumerant = &info.enumerants[value as usize];

                syn::parse_quote!(#type_path::#enumerant)
            }
            _ => unimplemented!(),
        };

        Ok(expr)
    }

    fn generate_enum(
        &self,
        schema: &EnumSchema<'_>,
        EnumInfo {
            enum_ident,
            enumerants,
            ..
        }: &EnumInfo,
        _: &mut FileContext,
    ) -> Result<GeneratedEnum> {
        Ok(GeneratedEnum {
            id: schema.node.id(),
            name: enum_ident.clone(),
            enumerants: enumerants.clone(),
        })
    }

    fn generate_const(
        &self,
        schema: &ConstSchema<'_>,
        info: &ConstInfo,
        ctx: &mut FileContext,
    ) -> Result<GeneratedConst> {
        let t = schema.value_type()?;
        let v = schema.value();
        Ok(GeneratedConst {
            ident: info.ident.clone(),
            const_type: self.resolve_type(&info.scope, &t, TypeContext::Const, ctx)?,
            value: self.generate_const_value(&info.scope, &t, &v)?,
        })
    }

    fn generate_const_value(
        &self,
        scope: &TypeScope,
        type_info: &Type<'_>,
        value: &ReaderOf<'_, schema_capnp::Value>,
    ) -> Result<Box<syn::Expr>> {
        if type_info.is_data_field() {
            return self.generate_data_value(scope, &type_info.kind, value);
        }

        let ptr = if type_info.is_list() {
            value
                .list()
                .field()
                .map_or_else(any::PtrReader::default, |f| f.ptr())
        } else {
            // Extract the correct pointer value field.
            match type_info.kind {
                TypeKind::Text => value
                    .text()
                    .field()
                    .map_or_else(any::PtrReader::default, |f| f.ptr()),
                TypeKind::Data => value
                    .data()
                    .field()
                    .map_or_else(any::PtrReader::default, |f| f.ptr()),
                TypeKind::Struct { .. } => value
                    .r#struct()
                    .field()
                    .map_or_else(any::PtrReader::default, |f| f.ptr()),
                TypeKind::Interface { .. } => any::PtrReader::default(),
                _ => value
                    .any_pointer()
                    .field()
                    .map_or_else(any::PtrReader::default, |f| f.ptr()),
            }
        };

        let expr = if ptr.is_null() {
            syn::parse_quote!(_p::ty::ConstPtr::null())
        } else {
            let slice = ptr_to_slice(&ptr);
            let words = words_lit(&slice);

            syn::parse_quote!(unsafe { _p::ty::ConstPtr::new(#words) })
        };

        Ok(expr)
    }

    fn generate_interface(
        &self,
        schema: &InterfaceSchema<'_>,
        info: &InterfaceInfo,
        ctx: &mut FileContext,
    ) -> Result<GeneratedInterface> {
        let interface_id = schema.node.id();
        let interface_str = schema.node().display_name()
            .get()
            .as_str()
            .map_err(|error| GeneratorError::InvalidName {
                context: NameContext::Node(interface_id),
                error,
            })?;

        let mut generated = GeneratedInterface {
            name: String::from(interface_str),
            mod_ident: info.mod_ident.clone(),
            client_ident: info.client_ident.clone(),
            client_methods: Vec::new(),
            server_ident: info.server_ident.clone(),
            server_supertraits: Vec::new(),
            server_methods: Vec::new(),
            dispatch_ident: info.dispatch_ident.clone(),
            dispatch_interfaces: Vec::new(),
            nested_items: Vec::new(),
        };

        for nested in schema.nested_items() {
            let nested = nested?;
            if let Some(item) = self.generate_item(nested.id, nested.item, ctx)? {
                generated.nested_items.push(item);
            }
        }

        let mut method_idents = IdentifierSet::new();

        for (idx, method) in schema.info.methods().into_iter().enumerate() {
            let method_id = idx as u16;
            let name_str = method.name()
                .get()
                .as_str()
                .map_err(|error| GeneratorError::InvalidName {
                    context: NameContext::Method {
                        type_id: interface_id,
                        index: method_id as u32,
                    },
                    error,
                })?;
            let name_ident = method_idents.make_unique(AsSnakeCase(name_str))?;

            let param_type_id = method.param_struct_type();
            let param_schema: StructSchema<'_> = schema.loader().try_schema(param_type_id)?;
            let param_info = self.find_info(param_type_id)?.unwrap_struct();
            let param_path = resolve_path(&param_info.struct_ident, &param_info.scope, &info.scope);

            let result_type_id = method.result_struct_type();
            let result_schema: StructSchema<'_> = schema.loader().try_schema(result_type_id)?;
            let result_info = self.find_info(result_type_id)?.unwrap_struct();
            let result_path = resolve_path(&result_info.struct_ident, &result_info.scope, &info.scope);

            let req_type;
            let req_func;
            let ctx_type;
            let ctx_func;

            // The streaming type ID
            if result_type_id == 0x995f9a3377c0b16e {
                req_type = quote!(::recapn_rpc::client::StreamingRequest<#param_path>);
                req_func = quote!(streaming_call);
                ctx_type = quote!(::recapn_rpc::server::StreamContext<#param_path>);
                ctx_func = quote!(into_stream_context);
            } else {
                req_type = quote!(::recapn_rpc::client::Request<#param_path, #result_path>);
                req_func = quote!(call);
                ctx_type = quote!(::recapn_rpc::server::CallContext<#param_path, #result_path>);
                ctx_func = quote!(into_call_context);
            }

            generated.client_methods.push(syn::parse_quote! {
                pub fn #name_ident(&self) -> #req_type {
                    self.0.#req_func(#interface_id, #method_id)
                }
            });
            generated.server_methods.push({
                let err_msg = format!("'{interface_str}.{name_str}' is not implemented for this type");
                syn::parse_quote! {
                    fn #name_ident(&mut self, ctx: #ctx_type) -> ::recapn_rpc::server::CallResult {
                        ctx.response.error(::recapn_rpc::Error::unimplemented(#err_msg))
                    }
                }
            });
            generated.dispatch_interfaces.push(syn::parse_quote! {
                (#interface_id, #method_id) => self.0.#name_ident(request.#ctx_func()).into(),
            });
            if param_schema.node().scope_id() == 0 {
                generated.nested_items.push({
                    let item = self.generate_struct(&param_schema, param_info, ctx)?;
                    GeneratedItem::Struct(item)
                });
            }
            if result_schema.node().scope_id() == 0 {
                generated.nested_items.push({
                    let item = self.generate_struct(&result_schema, result_info, ctx)?;
                    GeneratedItem::Struct(item)
                });
            }
        }

        generated.dispatch_interfaces.push(syn::parse_quote! {
            (#interface_id, _) => request.unimplemented_method(Self::NAME),
        });

        Ok(generated)
    }
}

fn words_lit(words: &[Word]) -> TokenStream {
    let words = words.iter().map(|Word([b0, b1, b2, b3, b4, b5, b6, b7])| {
        quote!(_p::Word([#b0, #b1, #b2, #b3, #b4, #b5, #b6, #b7]))
    });

    quote!(&[#(#words),*])
}

fn text_to_expr(text: text::Reader<'_>) -> Box<syn::Expr> {
    let bytes = syn::LitByteStr::new(text.as_bytes_with_nul(), proc_macro2::Span::call_site());

    syn::parse_quote!(_p::text::Reader::from_slice(#bytes))
}

fn data_to_expr(data: data::Reader<'_>) -> Box<syn::Expr> {
    let bytes = syn::LitByteStr::new(data.as_ref(), proc_macro2::Span::call_site());

    syn::parse_quote!(_p::data::Reader::from_slice(#bytes))
}

fn generate_untyped_ptr_value(s: &any::PtrReader<'_>) -> Box<syn::Expr> {
    let slice = ptr_to_slice(s);
    let words = words_lit(&slice);

    syn::parse_quote!(unsafe { _p::PtrReader::slice_unchecked(#words) })
}

fn ptr_to_slice(p: &any::PtrReader<'_>) -> Box<[Word]> {
    let size = p
        .target_size()
        .expect("failed to calculate size of struct default");
    assert_eq!(size.caps, 0, "default value contains caps!");
    assert!(
        size.words < u64::from(SegmentOffset::MAX_VALUE),
        "default value is too large to fit in a single segment!"
    );

    let size = AllocLen::new(size.words as u32 + 1).unwrap();
    let mut space = vec![Word::NULL; size.get() as usize].into_boxed_slice();
    let segment = Segment {
        data: NonNull::new(space.as_mut_ptr()).unwrap(),
        len: size.into(),
    };
    let alloc = unsafe { alloc::Scratch::with_segment(segment, alloc::Never) };

    let mut message = recapn::message::Message::new(alloc);
    let mut builder = message.builder();
    builder
        .by_ref()
        .into_root()
        .try_set(p, false, UnwrapErrors)
        .unwrap();

    let result = builder.segments().first();
    assert_eq!(
        result.len(),
        size.get(),
        "written struct value doesn't match size of original"
    );

    space
}

fn generate_untyped_struct_value(s: &any::StructReader<'_>) -> Box<syn::Expr> {
    let slice = struct_to_slice(s);
    let words = words_lit(slice.split_first().unwrap().1);
    let StructSize { data, ptrs } = s.as_ref().size();
    let size = quote!(_p::StructSize { data: #data, ptrs: #ptrs });

    syn::parse_quote!(unsafe { _p::StructReader::slice_unchecked(#words, #size) })
}

/// Deep-clone a struct into an array of words without the root pointer.
fn struct_to_slice(s: &any::StructReader<'_>) -> Box<[Word]> {
    let size = s.total_size().expect("failed to calculate size of struct");
    assert_eq!(size.caps, 0, "struct contains caps!");
    assert!(
        size.words < u64::from(SegmentOffset::MAX_VALUE),
        "struct is too large to fit in a single segment!"
    );

    let size = AllocLen::new(size.words as u32 + 1).unwrap();
    let mut space = vec![Word::NULL; size.get() as usize].into_boxed_slice();
    let segment = Segment {
        data: NonNull::new(space.as_mut_ptr()).unwrap(),
        len: size.into(),
    };
    let alloc = unsafe { alloc::Scratch::with_segment(segment, alloc::Never) };

    let mut message = recapn::message::Message::new(alloc);
    let mut builder = message.builder();
    builder
        .by_ref()
        .into_root()
        .try_set_any_struct(s, UnwrapErrors)
        .unwrap();

    let result = builder.segments().first();
    assert_eq!(
        result.len(),
        size.get(),
        "written struct value doesn't match size of original"
    );

    space
}

fn generate_untyped_list_value(list: &any::ListReader<'_>) -> Box<syn::Expr> {
    let result = list_to_slice(list);
    // Remove the root pointer since we're going to create a direct pointer
    // to the data itself which must be allocated immediately after it.
    let words = words_lit(result.split_first().unwrap().1);
    let len = list.len();
    let value_element_size = list.as_ref().element_size();

    let element_size_quote = element_size_to_tokens(value_element_size);

    syn::parse_quote!(unsafe { _p::ListReader::slice_unchecked(#words, #len, #element_size_quote) })
}

/// Deep-clone a list into an array of words without the root pointer.
fn list_to_slice(l: &any::ListReader<'_>) -> Box<[Word]> {
    let size = l
        .total_size()
        .expect("failed to calculate size of list value");
    assert_eq!(size.caps, 0, "list contains caps!");
    assert!(
        size.words < u64::from(SegmentOffset::MAX_VALUE),
        "list is too large to fit in a single segment!"
    );

    let size = AllocLen::new(size.words as u32 + 1).unwrap();
    let mut space = vec![Word::NULL; size.get() as usize].into_boxed_slice();
    let segment = Segment {
        data: NonNull::new(space.as_mut_ptr()).unwrap(),
        len: size.into(),
    };
    let alloc = unsafe { alloc::Scratch::with_segment(segment, alloc::Never) };

    let mut message = recapn::message::Message::new(alloc);
    let mut builder = message.builder();
    builder
        .by_ref()
        .into_root()
        .try_set_any_list(l, UnwrapErrors)
        .unwrap();

    let result = builder.segments().first();
    assert_eq!(
        result.len(),
        size.get(),
        "written struct value doesn't match size of original"
    );

    space
}

fn element_size_to_tokens(s: ElementSize) -> TokenStream {
    match s {
        ElementSize::Void => quote!(_p::ElementSize::Void),
        ElementSize::Bit => quote!(_p::ElementSize::Bit),
        ElementSize::Byte => quote!(_p::ElementSize::Byte),
        ElementSize::TwoBytes => quote!(_p::ElementSize::TwoBytes),
        ElementSize::FourBytes => quote!(_p::ElementSize::FourBytes),
        ElementSize::EightBytes => quote!(_p::ElementSize::EightBytes),
        ElementSize::Pointer => quote!(_p::ElementSize::Pointer),
        ElementSize::InlineComposite(StructSize { data, ptrs }) => {
            quote!(_p::ElementSize::InlineComposite(_p::StructSize { data: #data, ptrs: #ptrs }))
        }
    }
}
