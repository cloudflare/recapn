//! Provides most of the types that ultimately get turned into quoted code.
//!
//! This seperation makes it really easy to identify what's needed to generate
//! everything related to some item like a struct, field, enum, etc.
//!
//! If you want to add something to generated code, add it here first to figure out
//! the code layout and identify what parameters you'll need, then modify the generator
//! to provide it.

use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use recapn::ptr::StructSize;

/// A simple macro that automatically writes a `ToTokens` implementation based on a function
/// that returns a `TokenStream`
macro_rules! to_tokens {
    ($ty:ty |$self:ident| $block:block) => {
        impl ToTokens for $ty {
            fn to_token_stream($self: &Self) -> TokenStream $block

            fn to_tokens(&self, tokens: &mut TokenStream) {
                tokens.extend(self.to_token_stream())
            }

            fn into_token_stream(self) -> TokenStream
                where
                    Self: Sized,
            {
                self.to_token_stream()
            }
        }
    };
}

pub struct GeneratedRoot {
    pub files: Vec<GeneratedRootFile>,
}

to_tokens!(
    GeneratedRoot | self | {
        let Self { files } = self;
        quote! {
            #(#files)*
        }
    }
);

pub struct GeneratedRootFile {
    pub ident: syn::Ident,
    pub imports: Vec<syn::Ident>,
    pub path: String,
}

to_tokens!(
    GeneratedRootFile | self | {
        let Self {
            ident,
            imports,
            path,
        } = self;
        quote! {
            #[path = "."]
            pub mod #ident {
                #![allow(unused_imports)]
                use super::#ident as __file;
                mod __imports {
                    #(pub use super::super::#imports;)*
                }

                #[path = #path]
                mod #ident;
                pub use #ident::*;
            }
        }
    }
);

pub struct GeneratedFile {
    pub items: Vec<GeneratedItem>,
}

to_tokens!(
    GeneratedFile | self | {
        let Self { items, .. } = self;
        quote! {
            #![allow(unused, unsafe_code)]

            use recapn::prelude::gen as _p;
            use super::{__file, __imports};

            #(#items)*
        }
    }
);

pub enum GeneratedItem {
    Struct(GeneratedStruct),
    Enum(GeneratedEnum),
    Const(GeneratedConst),
}

impl ToTokens for GeneratedItem {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Self::Struct(s) => s.to_tokens(tokens),
            Self::Enum(e) => e.to_tokens(tokens),
            Self::Const(c) => c.to_tokens(tokens),
        }
    }
}

use crate::generator::GenericParam;

pub struct GeneratedStruct {
    pub ident: syn::Ident,
    pub mod_ident: syn::Ident,
    pub repr_type_param: syn::Ident,
    pub repr_field_ident: syn::Ident,
    /// A second type param used in cases where
    /// 
    /// * The repr type is used
    /// * It doesn't make sense to use the table type
    pub free_type_param: syn::Ident,
    pub table_type_param: syn::Ident,
    pub generic_params: Vec<GenericParam>,
    pub size: Option<StructSize>,
    pub fields: Vec<GeneratedField>,
    pub which: Option<GeneratedWhich>,
    pub nested_items: Vec<GeneratedItem>,
}

#[derive(Clone, Copy, Debug)]
pub enum FieldItem {
    Descriptor,
    RefAccessor,
    MutAccessor,
    OwnAccessor,
    Clear,
}

impl GeneratedStruct {
    pub fn generate_field_items(&self, item: FieldItem) -> impl Iterator<Item = TokenStream> + '_ {
        let fields = self.fields.iter().map(move |f| f.generate_item(item, self));
        let variants = self
            .which
            .as_ref()
            .map(move |w| w
                .fields
                .iter()
                .map(move |v| v.generate_item(item, w, self))
            ).into_iter().flatten();
        fields.chain(variants)
    }
    pub fn which_accessors(&self) -> Option<(TokenStream, TokenStream)> {
        let _ = self.which.as_ref()?;
        let modname = &self.mod_ident;
        let ref_accessor = quote! {
            #[inline]
            pub fn which(&self) -> Result<#modname::Which<&Self>, _p::NotInSchema> {
                unsafe { <#modname::Which<_> as _p::UnionViewer<_>>::get(self) }
            }
        };
        let mut_accessor = quote! {
            #[inline]
            pub fn which(&mut self) -> Result<#modname::Which<&mut Self>, _p::NotInSchema> {
                unsafe { <#modname::Which<_> as _p::UnionViewer<_>>::get(self) }
            }
        };
        Some((ref_accessor, mut_accessor))
    }
}

to_tokens!(GeneratedStruct |self| {
    let Self {
        ident: name,
        mod_ident: modname,
        repr_type_param,
        repr_field_ident,
        free_type_param,
        table_type_param,
        generic_params,
        nested_items: nested,
        size,
        which,
        ..
    } = self;
    let descriptors = self.generate_field_items(FieldItem::Descriptor);
    let ref_accessors = self.generate_field_items(FieldItem::RefAccessor);
    let mut_accessors = self.generate_field_items(FieldItem::MutAccessor);
    let own_accessors = self.generate_field_items(FieldItem::OwnAccessor);
    let (which_ref_accessor, which_mut_accessor) = self.which_accessors().unzip();
    let which_type = which.as_ref().map(|w| w.generate_type(self));

    let type_field_idents = generic_params.iter().map(|p| &p.field_name).collect::<Vec<_>>();
    let type_idents = generic_params.iter().map(|p| &p.type_name).collect::<Vec<_>>();

    let generic_params_list = quote!(#(#type_idents,)*);
    let generic_constrained_params = quote!(#(#type_idents: _p::FieldType,)*);
    let generic_constrained_params_default = quote!(#(#type_idents: _p::FieldType = _p::AnyPtr,)*);
    let generic_phantom_fields = generic_params.iter().map(
        |GenericParam { field_name, type_name }| quote! {
            #field_name: ::core::marker::PhantomData<fn() -> #type_name>
        }
    );

    let impl_a_table = quote!(impl<'a, #generic_constrained_params #table_type_param: _p::rpc::Table>);
    let struct_reader_a = quote!(_p::StructReader<'a, #table_type_param>);
    let reader_with_a = quote!(#modname::Reader<'a, #generic_params_list #table_type_param>);
    let struct_builder_a = quote!(_p::StructReader<'a, #table_type_param>);
    let builder_with_a = quote!(#modname::Builder<'a, #generic_params_list #table_type_param>);

    let table_constrained_param = quote!(#table_type_param: _p::rpc::Table,);

    let struct_group_marker = if let Some(StructSize { data, ptrs }) = size {
        quote! {
            impl<#generic_constrained_params> _p::ty::Struct for #name<#generic_params_list> {
                const SIZE: _p::StructSize = _p::StructSize { data: #data, ptrs: #ptrs };
            }
        }
    } else {
        let clears = self.generate_field_items(FieldItem::Clear);
        quote! {
            impl<#generic_constrained_params> _p::FieldGroup for #name<#generic_params_list> {
                unsafe fn clear<'a, 'b, #table_constrained_param>(s: &'a mut _p::StructBuilder<'b, #table_type_param>) {
                    #(#clears)*
                }
            }
        }
    };

    quote! {
        #[derive(Clone)]
        pub struct #name<#generic_constrained_params_default #repr_type_param = _p::Family> {
            #(#generic_phantom_fields,)*
            #repr_field_ident: #repr_type_param,
        }

        impl<#generic_constrained_params #repr_type_param> #name<#generic_params_list #repr_type_param> {
            fn wrap(#repr_field_ident: #repr_type_param) -> Self {
                Self {
                    #(#type_field_idents: ::core::marker::PhantomData,)*
                    #repr_field_ident,
                }
            }
        }

        impl<#generic_constrained_params #repr_type_param>
            _p::IntoFamily for
            #name<#generic_params_list #repr_type_param>
        {
            type Family = #name<#generic_params_list>;
        }

        impl<#generic_constrained_params #repr_type_param: _p::Capable>
            _p::Capable for
            #name<#generic_params_list #repr_type_param>
        {
            type Table = #repr_type_param::Table;

            type Imbued = #repr_type_param::Imbued;
            type ImbuedWith<#free_type_param: _p::rpc::Table> =
                #name<#generic_params_list #repr_type_param::ImbuedWith<#free_type_param>>;

            #[inline]
            fn imbued(&self) -> &Self::Imbued { self.#repr_field_ident.imbued() }

            #[inline]
            fn imbue_release<#free_type_param: _p::rpc::Table>(
                self,
                new_table: <Self::ImbuedWith<#free_type_param> as _p::Capable>::Imbued,
            ) -> (Self::ImbuedWith<#free_type_param>, Self::Imbued) {
                let (imbued, old) = self.#repr_field_ident.imbue_release(new_table);
                (#name::wrap(imbued), old)
            }

            #[inline]
            fn imbue_release_into<#free_type_param>(
                &self,
                other: #free_type_param,
            ) -> (#free_type_param::ImbuedWith<Self::Table>, #free_type_param::Imbued)
            where
                #free_type_param: _p::Capable,
                #free_type_param::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
            {
                self.#repr_field_ident.imbue_release_into(other)
            }
        }

        impl<#generic_constrained_params> _p::StructView for #name<#generic_params_list> {
            type Reader<'a, #table_type_param: _p::rpc::Table> = #reader_with_a;
            type Builder<'a, #table_type_param: _p::rpc::Table> = #builder_with_a;
        }

        #struct_group_marker

        impl<#generic_constrained_params> #name<#generic_params_list> {
            #(#descriptors)*
        }

        #impl_a_table _p::ty::TypedPtr for #reader_with_a {
            type Ptr = #struct_reader_a;
        }

        #impl_a_table ::core::convert::From<#struct_reader_a> for #reader_with_a {
            #[inline]
            fn from(ptr: #struct_reader_a) -> Self {
                Self::wrap(ptr)
            }
        }

        #impl_a_table ::core::convert::From<#reader_with_a> for #struct_reader_a {
            #[inline]
            fn from(reader: #reader_with_a) -> Self {
                reader.#repr_field_ident
            }
        }

        #impl_a_table ::core::convert::AsRef<#struct_reader_a> for #reader_with_a {
            #[inline]
            fn as_ref(&self) -> &#struct_reader_a {
                &self.#repr_field_ident
            }
        }

        #impl_a_table _p::ty::StructReader for #reader_with_a {}

        #impl_a_table #reader_with_a {
            #(#ref_accessors)*
            #which_ref_accessor
        }

        #impl_a_table _p::ty::TypedPtr for #builder_with_a {
            type Ptr = #struct_builder_a;
        }

        #impl_a_table ::core::convert::From<#builder_with_a> for #struct_builder_a {
            #[inline]
            fn from(reader: #builder_with_a) -> Self {
                reader.#repr_field_ident
            }
        }

        #impl_a_table ::core::convert::AsRef<#struct_builder_a> for #builder_with_a {
            #[inline]
            fn as_ref(&self) -> &#struct_builder_a {
                &self.#repr_field_ident
            }
        }

        #impl_a_table ::core::convert::AsMut<#struct_builder_a> for #builder_with_a {
            #[inline]
            fn as_mut(&mut self) -> &mut #struct_builder_a {
                &mut self.#repr_field_ident
            }
        }

        #impl_a_table _p::ty::StructBuilder for #builder_with_a {
            unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
                Self::wrap(ptr)
            }
        }

        #impl_a_table #builder_with_a {
            #(#mut_accessors)*
            #(#own_accessors)*
            #which_mut_accessor
        }

        pub mod #modname {
            use super::{__file, __imports, _p};

            pub type Reader<'a, #(#type_idents = _p::AnyPtr,)* #table_type_param = _p::rpc::Empty>
                = super::#name<#generic_params_list #struct_reader_a>;
            pub type Builder<'a, #(#type_idents = _p::AnyPtr,)* #table_type_param = _p::rpc::Empty>
                = super::#name<#generic_params_list #struct_builder_a>;

            #which_type

            #(#nested)*
        }
    }
});

pub struct GeneratedField {
    pub type_name: syn::Ident,
    pub field_type: Box<syn::Type>,
    pub accessor_ident: syn::Ident,
    /// The name for an accessor which consumes the builder, allowing a derived builder to be
    /// returned without referencing a local builder. This is the default behavior of capnp-rust's
    /// accessors. This field is an option to indicate if this field should have an own accessor
    /// at all. In the case of simple data fields, this will be None.
    pub own_accessor_ident: Option<syn::Ident>,
    pub descriptor_ident: syn::Ident,
    pub descriptor: Option<FieldDescriptor>,
}

pub struct FieldDescriptor {
    pub slot: u32,
    pub default: Box<syn::Expr>,
}

impl GeneratedField {
    fn descriptor_value(&self) -> TokenStream {
        let field_type = &self.field_type;
        self.descriptor
            .as_ref()
            .map(|FieldDescriptor { slot, default }| {
                quote!(_p::FieldInfo<#field_type> {
                    slot: #slot,
                    default: #default,
                })
            })
            .unwrap_or(quote!(()))
    }

    pub fn generate_item(&self, item: FieldItem, ty: &GeneratedStruct) -> TokenStream {
        let GeneratedStruct { table_type_param, generic_params, .. } = ty;
        let Self {
            accessor_ident,
            own_accessor_ident,
            field_type,
            type_name,
            descriptor_ident,
            ..
        } = self;

        let struct_generic_idents = generic_params.iter().map(|p| &p.type_name);
        let type_ref = quote!(#type_name::<#(#struct_generic_idents,)*>);

        match item {
            FieldItem::Descriptor => {
                let descriptor_value = self.descriptor_value();
                quote!(const #descriptor_ident: _p::Descriptor<#field_type> = #descriptor_value;)
            },
            FieldItem::RefAccessor => quote! {
                #[inline]
                pub fn #accessor_ident(&self) -> _p::Accessor<'_, 'a, #table_type_param, #field_type> {
                    unsafe { self.0.field::<#field_type>(&#type_ref::#descriptor_ident) }
                }
            },
            FieldItem::MutAccessor => quote! {
                #[inline]
                pub fn #accessor_ident(&mut self) -> _p::AccessorMut<'_, 'a, #table_type_param, #field_type> {
                    unsafe { self.0.field::<#field_type>(&#type_ref::#descriptor_ident) }
                }
            },
            FieldItem::OwnAccessor => {
                let Some(own_accessor_ident) = own_accessor_ident else {
                    return TokenStream::new()
                };

                quote! {
                    #[inline]
                    pub fn #own_accessor_ident(self) -> _p::AccessorOwned<'a, #table_type_param, #field_type> {
                        unsafe { self.0.into_field::<#field_type>(&#type_ref::#descriptor_ident) }
                    }
                }
            },
            FieldItem::Clear => quote! {
                s.clear_field::<#field_type>(&#type_ref::#descriptor_ident);
            },
        }
    }
}

pub struct GeneratedWhich {
    pub tag_slot: u32,
    /// The type parameter name used for the "view" type. For example, in
    /// 
    /// ```ignore
    /// pub enum Which<View: Viewable> {
    ///     Foo(ViewOf<i32>),
    ///     Bar(ViewOf<u64>),
    /// }
    /// ```
    /// 
    /// the parameter `View` is the view type.
    pub view_param: syn::Ident,
    /// Generic parameters scoped to this type. This is not the same as the generic parameters of
    /// the parent type, since this only includes parameters used by the union itself.
    /// 
    /// The names used here are re-used from the parent type, so they can be used when referring to
    /// the Which type in the parent's scope.
    pub generic_params: Vec<syn::Ident>,
    pub fields: Vec<GeneratedVariant>,
}

impl GeneratedWhich {
    pub fn call_clear(&self, ty: &GeneratedStruct) -> impl Iterator<Item = TokenStream> + '_ {
        let tag_slot = self.tag_slot as usize;
        let clear_tag = quote! {
            s.set_field_unchecked(#tag_slot, 0);
        };

        let clear_first = self
            .fields
            .iter()
            .find(|variant| variant.case == 0)
            .expect("no variant has case 0!")
            .generate_item(FieldItem::Clear, self, ty);

        [clear_tag, clear_first].into_iter()
    }

    pub fn generate_type(&self, ty: &GeneratedStruct) -> TokenStream {
        let GeneratedStruct {
            ident: struct_name,
            repr_field_ident,
            table_type_param,
            generic_params: struct_generic_params,
            ..
        } = ty;
        let GeneratedWhich {
            tag_slot,
            fields,
            view_param,
            generic_params,
        } = self;

        let struct_generic_idents = struct_generic_params
            .iter()
            .map(|p| &p.type_name)
            .collect::<Vec<_>>();
        let struct_generics = quote!(#(#struct_generic_idents,)*);
        let struct_generics_bounds = quote!(#(#struct_generic_idents: _p::field::FieldType,)*);

        let tag_slot = *tag_slot as usize;
        let disciminants = fields.iter().map(
            |GeneratedVariant {
                 discriminant_ident: name,
                 discriminant_field_type: field_type,
                 ..
             }| quote!(#name(_p::ViewOf<#view_param, #field_type>)),
        );
        let disciminant_matches = fields.iter()
            .map(|GeneratedVariant {
                discriminant_ident: name,
                discriminant_field_type: field_type,
                field: GeneratedField { descriptor_ident, .. },
                case,
            }| {
                quote!(#case => Ok(Which::#name(s.#repr_field_ident.field::<#field_type>(
                    &super::#struct_name::<#struct_generics>::#descriptor_ident.field
                ))))
            })
            .collect::<Vec<_>>();

        let table_bound = quote!(#table_type_param: _p::Table);
        let which_generic_defs = quote!(#(#generic_params: _p::field::FieldType = _p::AnyPtr,)*);

        let type_reader = quote!(&'b self::Reader<'a, #struct_generics #table_type_param>);
        let type_builder = quote!(&'b mut self::Builder<'a, #struct_generics #table_type_param>);

        quote! {
            pub enum Which<#which_generic_defs #view_param: _p::Viewable = _p::Family> {
                #(#disciminants),*
            }

            impl<'a, 'b, #struct_generics_bounds #table_bound>
                _p::UnionViewer<#type_reader> for Which<#(#generic_params,)*>
            {
                type View = Which<#(#generic_params,)* &'b _p::StructReader<'a, #table_type_param>>;

                unsafe fn get(s: #type_reader) -> Result<Self::View, _p::NotInSchema> {
                    let tag = s.#repr_field_ident.data_field::<u16>(#tag_slot);
                    match tag {
                        #(#disciminant_matches,)*
                        unknown => Err(_p::NotInSchema(unknown)),
                    }
                }
            }

            impl<'a, 'b, #struct_generics_bounds #table_bound>
                _p::UnionViewer<#type_builder> for Which<#(#generic_params,)*>
            {
                type View = Which<#(#generic_params,)* &'b mut _p::StructBuilder<'a, #table_type_param>>;

                unsafe fn get(s: #type_builder) -> Result<Self::View, _p::NotInSchema> {
                    let tag = s.#repr_field_ident.data_field::<u16>(#tag_slot);
                    match tag {
                        #(#disciminant_matches,)*
                        unknown => Err(_p::NotInSchema(unknown)),
                    }
                }
            }
        }
    }
}

pub struct GeneratedVariant {
    pub discriminant_ident: syn::Ident,
    /// The field type as written in the disciminant as `ViewOf<T, Type>`. This might be different
    /// from `GeneratedField::field_type` since the path resolution is different, as the Which type
    /// is defined in the type's module instead of the parent module.
    ///
    /// For example, given a struct A with a field that refers to union group B, field and accessors
    /// in the impls of A will refer to group B via `a::B`. However, since the Which type is defined
    /// in the same place as B, it _doesn't_ refer to it with the `a` mod in the path. Instead
    /// it refers to it directly with `B`.
    pub discriminant_field_type: Box<syn::Type>,
    pub case: u16,
    pub field: GeneratedField,
}

impl GeneratedVariant {
    pub fn generate_item(
        &self,
        item: FieldItem,
        which: &GeneratedWhich,
        ty: &GeneratedStruct,
    ) -> TokenStream {
        let GeneratedWhich { tag_slot, .. } = which;
        let GeneratedStruct { table_type_param, generic_params, .. } = ty;
        let Self {
            case,
            field: field @ GeneratedField {
                accessor_ident,
                own_accessor_ident,
                field_type,
                type_name,
                descriptor_ident,
                ..
            },
            ..
        } = self;

        let struct_generic_idents = generic_params.iter().map(|p| &p.type_name);
        let type_ref = quote!(#type_name::<#(#struct_generic_idents,)*>);

        match item {
            FieldItem::Descriptor => {
                let descriptor_value = field.descriptor_value();
                quote! {
                    const #descriptor_ident: _p::VariantDescriptor<#field_type>
                        = _p::VariantDescriptor::<#field_type> {
                            variant: _p::VariantInfo {
                                slot: #tag_slot,
                                case: #case,
                            },
                            field: #descriptor_value,
                        };
                }
            },
            FieldItem::RefAccessor => quote! {
                #[inline]
                pub fn #accessor_ident(&self) -> _p::Variant<'_, 'a, #table_type_param, #field_type> {
                    unsafe { self.0.variant::<#field_type>(&#type_ref::#descriptor_ident) }
                }
            },
            FieldItem::MutAccessor => quote! {
                #[inline]
                pub fn #accessor_ident(&mut self) -> _p::VariantMut<'_, 'a, #table_type_param, #field_type> {
                    unsafe { self.0.variant::<#field_type>(&#type_ref::#descriptor_ident) }
                }
            },
            FieldItem::OwnAccessor => {
                let Some(own_accessor_ident) = own_accessor_ident else {
                    return TokenStream::new()
                };

                quote! {
                    #[inline]
                    pub fn #own_accessor_ident(self) -> _p::VariantOwned<'a, #table_type_param, #field_type> {
                        unsafe { self.0.into_variant::<#field_type>(&#type_ref::#descriptor_ident) }
                    }
                }
            },
            FieldItem::Clear => quote! {
                s.clear_field::<#field_type>(&#type_ref::#descriptor_ident.field);
            },
        }
    }
}

pub struct GeneratedEnum {
    pub name: syn::Ident,
    pub enumerants: Vec<syn::Ident>,
}

to_tokens!(
    GeneratedEnum |self| {
        let (name, enumerants) = (&self.name, &self.enumerants);
        let enum_matches = self.enumerants.iter().enumerate().map(|(value, ident)| {
            let value = value as u16;
            quote!(#value => Ok(Self::#ident))
        });

        quote! {
            #[repr(u16)]
            #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
            pub enum #name {
                #[default]
                #(#enumerants),*
            }

            impl core::convert::TryFrom<u16> for #name {
                type Error = _p::NotInSchema;

                #[inline]
                fn try_from(value: u16) -> Result<Self, _p::NotInSchema> {
                    match value {
                        #(#enum_matches,)*
                        value => Err(_p::NotInSchema(value))
                    }
                }
            }

            impl core::convert::From<#name> for u16 {
                #[inline]
                fn from(value: #name) -> Self {
                    value as u16
                }
            }

            impl _p::ty::Enum for #name {}
        }
    }
);

pub struct GeneratedConst {
    pub ident: syn::Ident,
    pub const_type: Box<syn::Type>,
    pub value: Box<syn::Expr>,
}

to_tokens!(
    GeneratedConst |self| {
        let Self {
            ident,
            const_type,
            value,
        } = self;
        quote!(pub const #ident: #const_type = #value;)
    }
);