use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::ptr::NonNull;

use recapn::alloc::{self, AllocLen, Segment, Word};
use recapn::ptr::{PtrReader, ReturnErrors, StructSize};
use recapn::{any, text, NotInSchema, ReaderOf};

pub use crate::generated::capnp_schema_capnp as schema_capnp;

use crate::Result;
use schema_capnp::{
    node::{Annotation, Const, Enum, Interface, Struct, Which as NodeKind},
    Brand, Field, Node,
};

use self::schema_capnp::Value;

struct UncheckedNode {
    data: Box<[Word]>,
}

impl UncheckedNode {
    pub fn new(node: &ReaderOf<'_, Node>) -> Result<Self> {
        let size = node.as_ref().total_size()?;
        if size.caps != 0 {
            return Err(recapn::Error::CapabilityNotAllowed)?;
        }

        let Some(len) = size
            .words
            .checked_add(1)
            .and_then(|s| u32::try_from(s).ok())
            .and_then(AllocLen::new)
        else {
            return Err(SchemaError::NodeTooLarge)?;
        };

        let mut space = vec![Word::NULL; len.get() as usize].into_boxed_slice();
        let segment = Segment {
            data: NonNull::new(space.as_mut_ptr()).unwrap(),
            len: len.into(),
        };
        let alloc = unsafe { alloc::Scratch::with_segment(segment, alloc::Never) };

        let mut message = recapn::message::Message::new(alloc);
        let mut builder = message.builder();
        builder
            .by_ref()
            .into_root()
            .try_set_struct::<Node, _, _>(node, ReturnErrors)?;

        Ok(UncheckedNode { data: space })
    }

    pub fn get(&self) -> ReaderOf<'_, Node> {
        let data = NonNull::new(self.data.as_ptr().cast_mut()).unwrap();
        let ptr = unsafe { PtrReader::new_unchecked(data) };
        any::PtrReader::from(ptr).read_as_struct::<Node>()
    }
}

#[derive(Debug)]
pub enum SchemaError {
    NodeTooLarge,
    DuplicateNode(Id),
    MissingNode(Id),
    InvalidNestedNode(Id),
    UnknownNodeType(NotInSchema),
    UnknownFieldType(NotInSchema),
    UnknownType(NotInSchema),
    UnknownAnyType(NotInSchema),
    UnknownPointerType(NotInSchema),
    UnknownScopeBinding(NotInSchema),
    UnexpectedNodeType,
    InvalidBrandType,
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::NodeTooLarge => write!(f, "node too large"),
            SchemaError::DuplicateNode(id) => write!(f, "duplicate node {}", id),
            SchemaError::MissingNode(id) => write!(f, "missing node {}", id),
            SchemaError::InvalidNestedNode(id) => write!(f, "invalid nested node {}", id),
            SchemaError::UnknownNodeType(NotInSchema(variant)) => {
                write!(f, "unknown node type {}", variant)
            }
            SchemaError::UnknownType(NotInSchema(variant)) => write!(f, "unknown type {}", variant),
            SchemaError::UnknownFieldType(NotInSchema(variant)) => {
                write!(f, "unknown field type {}", variant)
            }
            SchemaError::UnknownAnyType(NotInSchema(variant)) => {
                write!(f, "unknown any pointer type {}", variant)
            }
            SchemaError::UnknownPointerType(NotInSchema(variant)) => {
                write!(f, "unknown unconstrained pointer type {}", variant)
            }
            SchemaError::UnknownScopeBinding(NotInSchema(variant)) => {
                write!(f, "unknown scope binding {}", variant)
            }
            SchemaError::UnexpectedNodeType => write!(f, "unexpected node type"),
            SchemaError::InvalidBrandType => write!(f, "invalid brand type"),
        }
    }
}

impl std::error::Error for SchemaError {}

pub type Id = u64;

/// Allows easily reading a schema from a set of nodes.
pub struct SchemaLoader {
    nodes: HashMap<u64, UncheckedNode>,
}

impl SchemaLoader {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
        }
    }

    pub fn load(&mut self, node: &ReaderOf<'_, Node>) -> Result<()> {
        let id = node.id();
        if self.nodes.contains_key(&id) {
            return Err(SchemaError::DuplicateNode(id))?;
        }

        let node = UncheckedNode::new(node)?;
        let _ = self.nodes.insert(id, node);

        Ok(())
    }

    pub fn files(&self) -> impl Iterator<Item = FileSchema<'_>> + '_ {
        self.nodes
            .values()
            .filter_map(|n| FileSchema::from_node(n.get(), self))
    }

    pub fn get(&self, id: Id) -> Option<ReaderOf<'_, Node>> {
        self.nodes.get(&id).map(UncheckedNode::get)
    }

    pub fn schema<'a, T: Schema<'a>>(&'a self, id: Id) -> Option<T> {
        self.get(id).and_then(|n| T::from_node(n, self))
    }

    pub fn try_schema<'a, T: Schema<'a>>(&'a self, id: Id) -> Result<T> {
        let node = self.get(id).ok_or(SchemaError::MissingNode(id))?;
        let schema = T::from_node(node, self).ok_or(SchemaError::UnexpectedNodeType)?;
        Ok(schema)
    }

    pub fn resolve_brand<'a>(&'a self, brand: &ReaderOf<'a, Brand>) -> Result<Branding<'_>> {
        let scopes_list = brand.scopes().get();
        let scopes = {
            let mut scopes = Vec::with_capacity(scopes_list.len() as usize);
            for scope in scopes_list {
                let scope_id = scope.scope_id();
                use schema_capnp::brand::scope::Which;
                let branding = match scope.which().map_err(SchemaError::UnknownScopeBinding)? {
                    Which::Bind(bind) => {
                        let mut bindings = Vec::new();
                        for (index, binding) in bind.into_iter().enumerate() {
                            use schema_capnp::brand::binding::Which;
                            let binding =
                                match binding.which().map_err(SchemaError::UnknownScopeBinding)? {
                                    Which::Type(ty) => {
                                        let ty = ty.get();
                                        self.resolve_type(&ty)?
                                    }
                                    Which::Unbound(()) => TypeKind::ScopeBound {
                                        scope: scope_id,
                                        index: index as u16,
                                    }
                                    .into(),
                                };
                            bindings.push(binding);
                        }
                        ScopeBranding::Bind(bindings)
                    }
                    Which::Inherit(()) => ScopeBranding::Inherit,
                };
                scopes.push((scope_id, branding));
            }
            scopes.sort_by_key(|(id, _)| *id);
            scopes
        };
        Ok(Branding { scopes })
    }

    pub fn gather_node_parameters<'a>(
        &'a self,
        node: &ReaderOf<'a, Node>,
    ) -> Result<VecDeque<Parameter<'a>>> {
        let mut vec = VecDeque::new();
        let mut node = node.clone();
        while node.is_generic() {
            let scope = node.id();
            for (idx, p) in node.parameters().into_iter().enumerate().rev() {
                vec.push_front(Parameter {
                    scope,
                    index: idx as u16,
                    name: p.name().get(),
                });
            }

            let parent = node.scope_id();
            node = self.get(parent).ok_or(SchemaError::MissingNode(parent))?;
        }

        Ok(vec)
    }

    pub fn gather_node_bindings<'a>(
        &'a self,
        node: &ReaderOf<'a, Node>,
        brand: &Branding<'a>,
    ) -> Result<VecDeque<Type<'a>>> {
        let mut vec = VecDeque::new();
        let mut node = node.clone();
        while node.is_generic() {
            let scope = node.id();
            let params = node.parameters().get();
            match brand.find(scope) {
                Some(ScopeBranding::Inherit) => {
                    for (idx, _) in params.into_iter().enumerate().rev() {
                        vec.push_front(
                            TypeKind::ScopeBound {
                                scope,
                                index: idx as u16,
                            }
                            .into(),
                        );
                    }
                }
                Some(ScopeBranding::Bind(bindings)) => {
                    assert_eq!(bindings.len(), params.len() as usize);
                    bindings.iter().for_each(|b| vec.push_front(b.clone()));
                }
                None => {
                    for _ in 0..params.len() {
                        // no binding was found, so we just push any pointer for each generic
                        vec.push_front(TypeKind::AnyPointer.into())
                    }
                }
            }

            let parent = node.scope_id();
            node = self.get(parent).ok_or(SchemaError::MissingNode(parent))?;
        }

        Ok(vec)
    }

    pub fn resolve_type<'a>(&'a self, ty: &ReaderOf<'a, schema_capnp::Type>) -> Result<Type<'a>> {
        self.resolve_type_inner(ty, 0)
    }

    fn resolve_type_inner<'a>(
        &'a self,
        ty: &ReaderOf<'a, schema_capnp::Type>,
        list_depth: u32,
    ) -> Result<Type<'a>> {
        use schema_capnp::r#type::Which;
        let kind = match ty.which().map_err(SchemaError::UnknownType)? {
            Which::Void(()) => TypeKind::Void,
            Which::Bool(()) => TypeKind::Bool,
            Which::Int8(()) => TypeKind::Int8,
            Which::Int16(()) => TypeKind::Int16,
            Which::Int32(()) => TypeKind::Int32,
            Which::Int64(()) => TypeKind::Int64,
            Which::Uint8(()) => TypeKind::Uint8,
            Which::Uint16(()) => TypeKind::Uint16,
            Which::Uint32(()) => TypeKind::Uint32,
            Which::Uint64(()) => TypeKind::Uint64,
            Which::Float32(()) => TypeKind::Float32,
            Which::Float64(()) => TypeKind::Float64,
            Which::Text(()) => TypeKind::Text,
            Which::Data(()) => TypeKind::Data,
            Which::List(list) => {
                let element_type = list.element_type().get();
                return self.resolve_type_inner(&element_type, list_depth + 1);
            }
            Which::Enum(e) => {
                let ty = self.try_schema(e.type_id())?;
                let brand = self.resolve_brand(&e.brand().get())?;
                TypeKind::Enum {
                    schema: Box::new(ty),
                    parameters: brand,
                }
            }
            Which::Struct(s) => {
                let ty = self.try_schema(s.type_id())?;
                let brand = self.resolve_brand(&s.brand().get())?;
                TypeKind::Struct {
                    schema: Box::new(ty),
                    parameters: brand,
                }
            }
            Which::Interface(i) => {
                let ty = self.try_schema(i.type_id())?;
                let brand = self.resolve_brand(&i.brand().get())?;
                TypeKind::Interface {
                    schema: Box::new(ty),
                    parameters: brand,
                }
            }
            Which::AnyPointer(any) => {
                use schema_capnp::r#type::any_pointer::Which;
                match any.which().map_err(SchemaError::UnknownPointerType)? {
                    Which::Unconstrained(unconstrained) => {
                        use schema_capnp::r#type::any_pointer::unconstrained::Which;
                        match unconstrained
                            .which()
                            .map_err(SchemaError::UnknownPointerType)?
                        {
                            Which::AnyKind(()) => TypeKind::AnyPointer,
                            Which::Struct(()) => TypeKind::AnyStruct,
                            Which::List(()) => TypeKind::AnyList,
                            Which::Capability(()) => TypeKind::AnyCapability,
                        }
                    }
                    Which::Parameter(param) => TypeKind::ScopeBound {
                        scope: param.scope_id(),
                        index: param.parameter_index(),
                    },
                    Which::ImplicitMethodParameter(method_param) => {
                        TypeKind::ImplicitMethodParameter {
                            index: method_param.parameter_index(),
                        }
                    }
                }
            }
        };
        Ok(Type { list_depth, kind })
    }
}

#[derive(Debug)]
pub struct Parameter<'a> {
    pub scope: Id,
    pub index: u16,
    pub name: text::Reader<'a>,
}

#[derive(Clone)]
pub enum ScopeBranding<'a> {
    Inherit,
    Bind(Vec<Type<'a>>),
}

#[derive(Clone)]
pub struct Branding<'a> {
    scopes: Vec<(Id, ScopeBranding<'a>)>,
}

impl<'a> Branding<'a> {
    pub fn find(&self, scope: Id) -> Option<&ScopeBranding<'a>> {
        let idx = self.scopes.binary_search_by(|(s, _)| s.cmp(&scope)).ok()?;
        Some(&self.scopes[idx].1)
    }
}

pub struct NamedNestedItem<'a> {
    pub name: text::Reader<'a>,
    pub id: Id,
    pub item: NestedItem<'a>,
}

pub enum NestedItem<'a> {
    Struct(StructSchema<'a>),
    Enum(EnumSchema<'a>),
    Interface(InterfaceSchema<'a>),
    Const(ConstSchema<'a>),
    Annotation(AnnotationSchema<'a>),
}

pub trait Schema<'a>: Sized {
    fn from_node(node: ReaderOf<'a, Node>, loader: &'a SchemaLoader) -> Option<Self>;

    fn node(&self) -> &ReaderOf<'a, Node>;
    fn loader(&self) -> &'a SchemaLoader;

    fn nested_items(&self) -> impl Iterator<Item = Result<NamedNestedItem<'a>>> + 'a {
        let loader = self.loader();
        self.node().nested_nodes().into_iter().map(|node| {
            let name = node.name().get();
            let id = node.id();
            let node = loader.get(id).ok_or(SchemaError::MissingNode(id))?;
            let kind = match node.which() {
                Ok(k) => k,
                Err(err) => Err(SchemaError::UnknownNodeType(err))?,
            };

            macro_rules! ctor {
                ($ty:ident, $info:expr) => {
                    $ty {
                        node,
                        info: $info,
                        loader,
                    }
                };
            }

            let item = match kind {
                NodeKind::File(()) => return Err(SchemaError::InvalidNestedNode(id))?,
                NodeKind::Struct(info) => NestedItem::Struct(ctor!(StructSchema, info)),
                NodeKind::Enum(info) => NestedItem::Enum(ctor!(EnumSchema, info)),
                NodeKind::Interface(info) => NestedItem::Interface(ctor!(InterfaceSchema, info)),
                NodeKind::Const(info) => NestedItem::Const(ctor!(ConstSchema, info)),
                NodeKind::Annotation(info) => NestedItem::Annotation(ctor!(AnnotationSchema, info)),
            };

            Ok(NamedNestedItem { name, id, item })
        })
    }

    /// Gather a list of all generic parameters in this node scope.
    fn gather_parameters(&self) -> Result<VecDeque<Parameter<'a>>> {
        self.loader().gather_node_parameters(self.node())
    }

    /// Gather a list of all generic bindings with the given brand.
    fn gather_bindings(&self, brand: &Branding<'a>) -> Result<VecDeque<Type<'a>>> {
        self.loader().gather_node_bindings(self.node(), brand)
    }
}

#[derive(Clone)]
pub struct Type<'a> {
    pub list_depth: u32,
    pub kind: TypeKind<'a>,
}

impl<'a> From<TypeKind<'a>> for Type<'a> {
    fn from(value: TypeKind<'a>) -> Self {
        Type {
            list_depth: 0,
            kind: value,
        }
    }
}

#[derive(Clone)]
pub enum TypeKind<'a> {
    Void,
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Float32,
    Float64,
    Enum {
        schema: Box<EnumSchema<'a>>,
        parameters: Branding<'a>,
    },
    Data,
    Text,
    Struct {
        schema: Box<StructSchema<'a>>,
        parameters: Branding<'a>,
    },
    Interface {
        schema: Box<InterfaceSchema<'a>>,
        parameters: Branding<'a>,
    },
    AnyPointer,
    AnyStruct,
    AnyList,
    AnyCapability,
    ScopeBound {
        scope: Id,
        index: u16,
    },
    ImplicitMethodParameter {
        index: u16,
    },
}

impl Type<'_> {
    pub fn is_list(&self) -> bool {
        self.list_depth > 0
    }

    pub fn is_list_of_lists(&self) -> bool {
        self.list_depth > 1
    }

    /// Returns whether a field of this type would be placed in the data section of a struct
    pub fn is_data_field(&self) -> bool {
        if self.list_depth != 0 {
            return false;
        }

        use TypeKind::*;
        matches!(
            &self.kind,
            Void | Bool
                | Int8
                | Int16
                | Int32
                | Int64
                | Uint8
                | Uint16
                | Uint32
                | Uint64
                | Float32
                | Float64
                | Enum { .. }
        )
    }

    pub fn is_ptr_field(&self) -> bool {
        !self.is_data_field()
    }
}

pub struct FileSchema<'a> {
    pub node: ReaderOf<'a, Node>,
    loader: &'a SchemaLoader,
}

impl<'a> Schema<'a> for FileSchema<'a> {
    #[inline]
    fn from_node(node: ReaderOf<'a, Node>, loader: &'a SchemaLoader) -> Option<Self> {
        node.file().is_set().then(|| Self { node, loader })
    }

    #[inline]
    fn node(&self) -> &ReaderOf<'a, Node> {
        &self.node
    }
    #[inline]
    fn loader(&self) -> &'a SchemaLoader {
        self.loader
    }
}

macro_rules! schema_type {
    ($schema:ident, $ty:ident, $fn:ident) => {
        #[derive(Clone)]
        pub struct $schema<'a> {
            pub node: ReaderOf<'a, Node>,
            // TODO(someday): This is the same as node. Avoid doubling up?
            pub info: ReaderOf<'a, $ty>,
            #[allow(dead_code)]
            loader: &'a SchemaLoader,
        }

        impl<'a> Schema<'a> for $schema<'a> {
            #[inline]
            fn from_node(node: ReaderOf<'a, Node>, loader: &'a SchemaLoader) -> Option<Self> {
                node.$fn().get().map(|info| Self { node, info, loader })
            }

            #[inline]
            fn node(&self) -> &ReaderOf<'a, Node> {
                &self.node
            }
            #[inline]
            fn loader(&self) -> &'a SchemaLoader {
                self.loader
            }
        }
    };
}

schema_type!(StructSchema, Struct, r#struct);

impl<'a> StructSchema<'a> {
    pub fn struct_size(&self) -> StructSize {
        StructSize {
            data: self.info.data_word_count(),
            ptrs: self.info.pointer_count(),
        }
    }

    pub fn is_group(&self) -> bool {
        self.info.is_group()
    }

    pub fn fields(&self) -> impl Iterator<Item = FieldSchema<'a, '_>> + '_ {
        self.info
            .fields()
            .into_iter()
            .map(|info| FieldSchema { parent: self, info })
    }
}

pub struct FieldSchema<'a, 'schema> {
    pub parent: &'schema StructSchema<'a>,
    pub info: ReaderOf<'a, Field>,
}

impl<'a> FieldSchema<'a, '_> {
    pub const NO_DISCRIMINANT: u16 = 0xffff;

    pub fn name(&self) -> text::Reader<'a> {
        self.info.name().get()
    }

    pub fn field_type(&self) -> Result<FieldType<'a>> {
        Ok(
            match self.info.which().map_err(SchemaError::UnknownFieldType)? {
                schema_capnp::field::Which::Slot(slot) => {
                    let ty = slot.r#type().get();
                    FieldType::Slot {
                        offset: slot.offset(),
                        field_type: self.parent.loader.resolve_type(&ty)?,
                        default_value: slot
                            .had_explicit_default()
                            .then(|| slot.default_value().get_option())
                            .flatten(),
                    }
                }
                schema_capnp::field::Which::Group(group) => {
                    let ty = self.parent.loader.try_schema(group.type_id())?;
                    FieldType::Group(ty)
                }
            },
        )
    }

    pub fn discriminant(&self) -> Option<u16> {
        let value = self.info.discriminant_value();
        if value == Self::NO_DISCRIMINANT {
            None
        } else {
            Some(value)
        }
    }
}

pub enum FieldType<'a> {
    Slot {
        offset: u32,
        field_type: Type<'a>,
        default_value: Option<ReaderOf<'a, Value>>,
    },
    Group(StructSchema<'a>),
}

schema_type!(EnumSchema, Enum, r#enum);

impl<'a> EnumSchema<'a> {
    pub fn enumerants(&self) -> impl Iterator<Item = text::Reader<'a>> + 'a {
        self.info.enumerants().into_iter().map(|e| e.name().get())
    }
}

schema_type!(InterfaceSchema, Interface, interface);

impl<'a> InterfaceSchema<'a> {}

schema_type!(ConstSchema, Const, r#const);

impl<'a> ConstSchema<'a> {
    pub fn value_type(&self) -> Result<Type<'a>> {
        let ty = self.info.r#type().get();
        self.loader.resolve_type(&ty)
    }

    pub fn value(&self) -> ReaderOf<'a, Value> {
        self.info.value().get()
    }
}

schema_type!(AnnotationSchema, Annotation, annotation);
