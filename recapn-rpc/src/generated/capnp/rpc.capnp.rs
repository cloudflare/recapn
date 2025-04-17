#![allow(unused, unsafe_code)]
use recapn::prelude::generated as _p;
use super::{__file, __imports};
#[derive(Clone)]
pub struct Message<T = _p::Family>(T);
impl _p::ty::SchemaType for Message {
    const ID: u64 = 10500036013887172658u64;
}
impl<T> _p::IntoFamily for Message<T> {
    type Family = Message;
}
impl<T: _p::Capable> _p::Capable for Message<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Message<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Message(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for message::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for message::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Message(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<message::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: message::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for message::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for message::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for message::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<message::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: message::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for message::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for message::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for message::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Message {
    type Reader<'a, T: _p::rpc::Table> = message::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = message::Builder<'a, T>;
}
impl _p::ty::Struct for Message {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 1u16,
    };
}
impl Message {
    const UNIMPLEMENTED: _p::VariantDescriptor<_p::Struct<Message>> = _p::VariantDescriptor::<
        _p::Struct<Message>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 0u16,
        },
        field: _p::Descriptor::<_p::Struct<Message>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const ABORT: _p::VariantDescriptor<_p::Struct<Exception>> = _p::VariantDescriptor::<
        _p::Struct<Exception>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 1u16,
        },
        field: _p::Descriptor::<_p::Struct<Exception>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const CALL: _p::VariantDescriptor<_p::Struct<Call>> = _p::VariantDescriptor::<
        _p::Struct<Call>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 2u16,
        },
        field: _p::Descriptor::<_p::Struct<Call>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const RETURN: _p::VariantDescriptor<_p::Struct<Return>> = _p::VariantDescriptor::<
        _p::Struct<Return>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 3u16,
        },
        field: _p::Descriptor::<_p::Struct<Return>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const FINISH: _p::VariantDescriptor<_p::Struct<Finish>> = _p::VariantDescriptor::<
        _p::Struct<Finish>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 4u16,
        },
        field: _p::Descriptor::<_p::Struct<Finish>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const RESOLVE: _p::VariantDescriptor<_p::Struct<Resolve>> = _p::VariantDescriptor::<
        _p::Struct<Resolve>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 5u16,
        },
        field: _p::Descriptor::<_p::Struct<Resolve>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const RELEASE: _p::VariantDescriptor<_p::Struct<Release>> = _p::VariantDescriptor::<
        _p::Struct<Release>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 6u16,
        },
        field: _p::Descriptor::<_p::Struct<Release>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const OBSOLETE_SAVE: _p::VariantDescriptor<_p::AnyPtr> = _p::VariantDescriptor::<
        _p::AnyPtr,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 7u16,
        },
        field: _p::Descriptor::<_p::AnyPtr> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const BOOTSTRAP: _p::VariantDescriptor<_p::Struct<Bootstrap>> = _p::VariantDescriptor::<
        _p::Struct<Bootstrap>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 8u16,
        },
        field: _p::Descriptor::<_p::Struct<Bootstrap>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const OBSOLETE_DELETE: _p::VariantDescriptor<_p::AnyPtr> = _p::VariantDescriptor::<
        _p::AnyPtr,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 9u16,
        },
        field: _p::Descriptor::<_p::AnyPtr> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const PROVIDE: _p::VariantDescriptor<_p::Struct<Provide>> = _p::VariantDescriptor::<
        _p::Struct<Provide>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 10u16,
        },
        field: _p::Descriptor::<_p::Struct<Provide>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const ACCEPT: _p::VariantDescriptor<_p::Struct<Accept>> = _p::VariantDescriptor::<
        _p::Struct<Accept>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 11u16,
        },
        field: _p::Descriptor::<_p::Struct<Accept>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const JOIN: _p::VariantDescriptor<_p::Struct<Join>> = _p::VariantDescriptor::<
        _p::Struct<Join>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 12u16,
        },
        field: _p::Descriptor::<_p::Struct<Join>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const DISEMBARGO: _p::VariantDescriptor<_p::Struct<Disembargo>> = _p::VariantDescriptor::<
        _p::Struct<Disembargo>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 13u16,
        },
        field: _p::Descriptor::<_p::Struct<Disembargo>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
}
impl<'p, T: _p::rpc::Table + 'p> message::Reader<'p, T> {
    #[inline]
    pub fn unimplemented(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Message>> {
        unsafe {
            <_p::Struct<
                Message,
            > as _p::field::FieldType>::variant(&self.0, &Message::UNIMPLEMENTED)
        }
    }
    #[inline]
    pub fn abort(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Exception>> {
        unsafe {
            <_p::Struct<
                Exception,
            > as _p::field::FieldType>::variant(&self.0, &Message::ABORT)
        }
    }
    #[inline]
    pub fn call(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Call>> {
        unsafe {
            <_p::Struct<Call> as _p::field::FieldType>::variant(&self.0, &Message::CALL)
        }
    }
    #[inline]
    pub fn r#return(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Return>> {
        unsafe {
            <_p::Struct<
                Return,
            > as _p::field::FieldType>::variant(&self.0, &Message::RETURN)
        }
    }
    #[inline]
    pub fn finish(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Finish>> {
        unsafe {
            <_p::Struct<
                Finish,
            > as _p::field::FieldType>::variant(&self.0, &Message::FINISH)
        }
    }
    #[inline]
    pub fn resolve(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Resolve>> {
        unsafe {
            <_p::Struct<
                Resolve,
            > as _p::field::FieldType>::variant(&self.0, &Message::RESOLVE)
        }
    }
    #[inline]
    pub fn release(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Release>> {
        unsafe {
            <_p::Struct<
                Release,
            > as _p::field::FieldType>::variant(&self.0, &Message::RELEASE)
        }
    }
    #[inline]
    pub fn obsolete_save(&self) -> _p::Variant<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::variant(
                &self.0,
                &Message::OBSOLETE_SAVE,
            )
        }
    }
    #[inline]
    pub fn bootstrap(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Bootstrap>> {
        unsafe {
            <_p::Struct<
                Bootstrap,
            > as _p::field::FieldType>::variant(&self.0, &Message::BOOTSTRAP)
        }
    }
    #[inline]
    pub fn obsolete_delete(&self) -> _p::Variant<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::variant(
                &self.0,
                &Message::OBSOLETE_DELETE,
            )
        }
    }
    #[inline]
    pub fn provide(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Provide>> {
        unsafe {
            <_p::Struct<
                Provide,
            > as _p::field::FieldType>::variant(&self.0, &Message::PROVIDE)
        }
    }
    #[inline]
    pub fn accept(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Accept>> {
        unsafe {
            <_p::Struct<
                Accept,
            > as _p::field::FieldType>::variant(&self.0, &Message::ACCEPT)
        }
    }
    #[inline]
    pub fn join(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Join>> {
        unsafe {
            <_p::Struct<Join> as _p::field::FieldType>::variant(&self.0, &Message::JOIN)
        }
    }
    #[inline]
    pub fn disembargo(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Disembargo>> {
        unsafe {
            <_p::Struct<
                Disembargo,
            > as _p::field::FieldType>::variant(&self.0, &Message::DISEMBARGO)
        }
    }
    #[inline]
    pub fn which(&self) -> Result<message::Which<&Self>, _p::NotInSchema> {
        unsafe { <message::Which<_> as _p::UnionViewer<_>>::get(self) }
    }
}
impl<'p, T: _p::rpc::Table + 'p> message::Builder<'p, T> {
    #[inline]
    pub fn unimplemented(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Message>> {
        unsafe {
            <_p::Struct<
                Message,
            > as _p::field::FieldType>::variant(&mut self.0, &Message::UNIMPLEMENTED)
        }
    }
    #[inline]
    pub fn abort(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Exception>> {
        unsafe {
            <_p::Struct<
                Exception,
            > as _p::field::FieldType>::variant(&mut self.0, &Message::ABORT)
        }
    }
    #[inline]
    pub fn call(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Call>> {
        unsafe {
            <_p::Struct<
                Call,
            > as _p::field::FieldType>::variant(&mut self.0, &Message::CALL)
        }
    }
    #[inline]
    pub fn r#return(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Return>> {
        unsafe {
            <_p::Struct<
                Return,
            > as _p::field::FieldType>::variant(&mut self.0, &Message::RETURN)
        }
    }
    #[inline]
    pub fn finish(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Finish>> {
        unsafe {
            <_p::Struct<
                Finish,
            > as _p::field::FieldType>::variant(&mut self.0, &Message::FINISH)
        }
    }
    #[inline]
    pub fn resolve(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Resolve>> {
        unsafe {
            <_p::Struct<
                Resolve,
            > as _p::field::FieldType>::variant(&mut self.0, &Message::RESOLVE)
        }
    }
    #[inline]
    pub fn release(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Release>> {
        unsafe {
            <_p::Struct<
                Release,
            > as _p::field::FieldType>::variant(&mut self.0, &Message::RELEASE)
        }
    }
    #[inline]
    pub fn obsolete_save(&mut self) -> _p::VariantMut<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::variant(
                &mut self.0,
                &Message::OBSOLETE_SAVE,
            )
        }
    }
    #[inline]
    pub fn bootstrap(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Bootstrap>> {
        unsafe {
            <_p::Struct<
                Bootstrap,
            > as _p::field::FieldType>::variant(&mut self.0, &Message::BOOTSTRAP)
        }
    }
    #[inline]
    pub fn obsolete_delete(&mut self) -> _p::VariantMut<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::variant(
                &mut self.0,
                &Message::OBSOLETE_DELETE,
            )
        }
    }
    #[inline]
    pub fn provide(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Provide>> {
        unsafe {
            <_p::Struct<
                Provide,
            > as _p::field::FieldType>::variant(&mut self.0, &Message::PROVIDE)
        }
    }
    #[inline]
    pub fn accept(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Accept>> {
        unsafe {
            <_p::Struct<
                Accept,
            > as _p::field::FieldType>::variant(&mut self.0, &Message::ACCEPT)
        }
    }
    #[inline]
    pub fn join(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Join>> {
        unsafe {
            <_p::Struct<
                Join,
            > as _p::field::FieldType>::variant(&mut self.0, &Message::JOIN)
        }
    }
    #[inline]
    pub fn disembargo(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Disembargo>> {
        unsafe {
            <_p::Struct<
                Disembargo,
            > as _p::field::FieldType>::variant(&mut self.0, &Message::DISEMBARGO)
        }
    }
    #[inline]
    pub fn into_unimplemented(self) -> _p::VariantOwned<'p, T, _p::Struct<Message>> {
        unsafe {
            <_p::Struct<
                Message,
            > as _p::field::FieldType>::variant(self.0, &Message::UNIMPLEMENTED)
        }
    }
    #[inline]
    pub fn into_abort(self) -> _p::VariantOwned<'p, T, _p::Struct<Exception>> {
        unsafe {
            <_p::Struct<
                Exception,
            > as _p::field::FieldType>::variant(self.0, &Message::ABORT)
        }
    }
    #[inline]
    pub fn into_call(self) -> _p::VariantOwned<'p, T, _p::Struct<Call>> {
        unsafe {
            <_p::Struct<Call> as _p::field::FieldType>::variant(self.0, &Message::CALL)
        }
    }
    #[inline]
    pub fn into_return(self) -> _p::VariantOwned<'p, T, _p::Struct<Return>> {
        unsafe {
            <_p::Struct<
                Return,
            > as _p::field::FieldType>::variant(self.0, &Message::RETURN)
        }
    }
    #[inline]
    pub fn into_finish(self) -> _p::VariantOwned<'p, T, _p::Struct<Finish>> {
        unsafe {
            <_p::Struct<
                Finish,
            > as _p::field::FieldType>::variant(self.0, &Message::FINISH)
        }
    }
    #[inline]
    pub fn into_resolve(self) -> _p::VariantOwned<'p, T, _p::Struct<Resolve>> {
        unsafe {
            <_p::Struct<
                Resolve,
            > as _p::field::FieldType>::variant(self.0, &Message::RESOLVE)
        }
    }
    #[inline]
    pub fn into_release(self) -> _p::VariantOwned<'p, T, _p::Struct<Release>> {
        unsafe {
            <_p::Struct<
                Release,
            > as _p::field::FieldType>::variant(self.0, &Message::RELEASE)
        }
    }
    #[inline]
    pub fn into_obsolete_save(self) -> _p::VariantOwned<'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::variant(
                self.0,
                &Message::OBSOLETE_SAVE,
            )
        }
    }
    #[inline]
    pub fn into_bootstrap(self) -> _p::VariantOwned<'p, T, _p::Struct<Bootstrap>> {
        unsafe {
            <_p::Struct<
                Bootstrap,
            > as _p::field::FieldType>::variant(self.0, &Message::BOOTSTRAP)
        }
    }
    #[inline]
    pub fn into_obsolete_delete(self) -> _p::VariantOwned<'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::variant(
                self.0,
                &Message::OBSOLETE_DELETE,
            )
        }
    }
    #[inline]
    pub fn into_provide(self) -> _p::VariantOwned<'p, T, _p::Struct<Provide>> {
        unsafe {
            <_p::Struct<
                Provide,
            > as _p::field::FieldType>::variant(self.0, &Message::PROVIDE)
        }
    }
    #[inline]
    pub fn into_accept(self) -> _p::VariantOwned<'p, T, _p::Struct<Accept>> {
        unsafe {
            <_p::Struct<
                Accept,
            > as _p::field::FieldType>::variant(self.0, &Message::ACCEPT)
        }
    }
    #[inline]
    pub fn into_join(self) -> _p::VariantOwned<'p, T, _p::Struct<Join>> {
        unsafe {
            <_p::Struct<Join> as _p::field::FieldType>::variant(self.0, &Message::JOIN)
        }
    }
    #[inline]
    pub fn into_disembargo(self) -> _p::VariantOwned<'p, T, _p::Struct<Disembargo>> {
        unsafe {
            <_p::Struct<
                Disembargo,
            > as _p::field::FieldType>::variant(self.0, &Message::DISEMBARGO)
        }
    }
    #[inline]
    pub fn which(&mut self) -> Result<message::Which<&mut Self>, _p::NotInSchema> {
        unsafe { <message::Which<_> as _p::UnionViewer<_>>::get(self) }
    }
}
pub mod message {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Message<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Message<_p::StructBuilder<'a, T>>;
    pub enum Which<T: _p::Viewable = _p::Family> {
        Unimplemented(_p::ViewOf<T, _p::Struct<super::Message>>),
        Abort(_p::ViewOf<T, _p::Struct<super::Exception>>),
        Call(_p::ViewOf<T, _p::Struct<super::Call>>),
        Return(_p::ViewOf<T, _p::Struct<super::Return>>),
        Finish(_p::ViewOf<T, _p::Struct<super::Finish>>),
        Resolve(_p::ViewOf<T, _p::Struct<super::Resolve>>),
        Release(_p::ViewOf<T, _p::Struct<super::Release>>),
        ObsoleteSave(_p::ViewOf<T, _p::AnyPtr>),
        Bootstrap(_p::ViewOf<T, _p::Struct<super::Bootstrap>>),
        ObsoleteDelete(_p::ViewOf<T, _p::AnyPtr>),
        Provide(_p::ViewOf<T, _p::Struct<super::Provide>>),
        Accept(_p::ViewOf<T, _p::Struct<super::Accept>>),
        Join(_p::ViewOf<T, _p::Struct<super::Join>>),
        Disembargo(_p::ViewOf<T, _p::Struct<super::Disembargo>>),
    }
    impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b Reader<'p, T>> for Which {
        type View = Which<&'b Reader<'p, T>>;
        unsafe fn get(repr: &'b Reader<'p, T>) -> Result<Self::View, _p::NotInSchema> {
            let tag = repr.0.data_field::<u16>(0usize);
            match tag {
                0u16 => {
                    Ok(
                        Which::Unimplemented(
                            <_p::Struct<
                                super::Message,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::UNIMPLEMENTED.field,
                            ),
                        ),
                    )
                }
                1u16 => {
                    Ok(
                        Which::Abort(
                            <_p::Struct<
                                super::Exception,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::ABORT.field,
                            ),
                        ),
                    )
                }
                2u16 => {
                    Ok(
                        Which::Call(
                            <_p::Struct<
                                super::Call,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::CALL.field,
                            ),
                        ),
                    )
                }
                3u16 => {
                    Ok(
                        Which::Return(
                            <_p::Struct<
                                super::Return,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::RETURN.field,
                            ),
                        ),
                    )
                }
                4u16 => {
                    Ok(
                        Which::Finish(
                            <_p::Struct<
                                super::Finish,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::FINISH.field,
                            ),
                        ),
                    )
                }
                5u16 => {
                    Ok(
                        Which::Resolve(
                            <_p::Struct<
                                super::Resolve,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::RESOLVE.field,
                            ),
                        ),
                    )
                }
                6u16 => {
                    Ok(
                        Which::Release(
                            <_p::Struct<
                                super::Release,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::RELEASE.field,
                            ),
                        ),
                    )
                }
                7u16 => {
                    Ok(
                        Which::ObsoleteSave(
                            <_p::AnyPtr as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::OBSOLETE_SAVE.field,
                            ),
                        ),
                    )
                }
                8u16 => {
                    Ok(
                        Which::Bootstrap(
                            <_p::Struct<
                                super::Bootstrap,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::BOOTSTRAP.field,
                            ),
                        ),
                    )
                }
                9u16 => {
                    Ok(
                        Which::ObsoleteDelete(
                            <_p::AnyPtr as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::OBSOLETE_DELETE.field,
                            ),
                        ),
                    )
                }
                10u16 => {
                    Ok(
                        Which::Provide(
                            <_p::Struct<
                                super::Provide,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::PROVIDE.field,
                            ),
                        ),
                    )
                }
                11u16 => {
                    Ok(
                        Which::Accept(
                            <_p::Struct<
                                super::Accept,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::ACCEPT.field,
                            ),
                        ),
                    )
                }
                12u16 => {
                    Ok(
                        Which::Join(
                            <_p::Struct<
                                super::Join,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::JOIN.field,
                            ),
                        ),
                    )
                }
                13u16 => {
                    Ok(
                        Which::Disembargo(
                            <_p::Struct<
                                super::Disembargo,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Message::DISEMBARGO.field,
                            ),
                        ),
                    )
                }
                unknown => Err(_p::NotInSchema(unknown)),
            }
        }
    }
    impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b mut Builder<'p, T>>
    for Which {
        type View = Which<&'b mut Builder<'p, T>>;
        unsafe fn get(
            repr: &'b mut Builder<'p, T>,
        ) -> Result<Self::View, _p::NotInSchema> {
            let tag = repr.0.data_field::<u16>(0usize);
            match tag {
                0u16 => {
                    Ok(
                        Which::Unimplemented(
                            <_p::Struct<
                                super::Message,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::UNIMPLEMENTED.field,
                            ),
                        ),
                    )
                }
                1u16 => {
                    Ok(
                        Which::Abort(
                            <_p::Struct<
                                super::Exception,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::ABORT.field,
                            ),
                        ),
                    )
                }
                2u16 => {
                    Ok(
                        Which::Call(
                            <_p::Struct<
                                super::Call,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::CALL.field,
                            ),
                        ),
                    )
                }
                3u16 => {
                    Ok(
                        Which::Return(
                            <_p::Struct<
                                super::Return,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::RETURN.field,
                            ),
                        ),
                    )
                }
                4u16 => {
                    Ok(
                        Which::Finish(
                            <_p::Struct<
                                super::Finish,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::FINISH.field,
                            ),
                        ),
                    )
                }
                5u16 => {
                    Ok(
                        Which::Resolve(
                            <_p::Struct<
                                super::Resolve,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::RESOLVE.field,
                            ),
                        ),
                    )
                }
                6u16 => {
                    Ok(
                        Which::Release(
                            <_p::Struct<
                                super::Release,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::RELEASE.field,
                            ),
                        ),
                    )
                }
                7u16 => {
                    Ok(
                        Which::ObsoleteSave(
                            <_p::AnyPtr as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::OBSOLETE_SAVE.field,
                            ),
                        ),
                    )
                }
                8u16 => {
                    Ok(
                        Which::Bootstrap(
                            <_p::Struct<
                                super::Bootstrap,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::BOOTSTRAP.field,
                            ),
                        ),
                    )
                }
                9u16 => {
                    Ok(
                        Which::ObsoleteDelete(
                            <_p::AnyPtr as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::OBSOLETE_DELETE.field,
                            ),
                        ),
                    )
                }
                10u16 => {
                    Ok(
                        Which::Provide(
                            <_p::Struct<
                                super::Provide,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::PROVIDE.field,
                            ),
                        ),
                    )
                }
                11u16 => {
                    Ok(
                        Which::Accept(
                            <_p::Struct<
                                super::Accept,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::ACCEPT.field,
                            ),
                        ),
                    )
                }
                12u16 => {
                    Ok(
                        Which::Join(
                            <_p::Struct<
                                super::Join,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::JOIN.field,
                            ),
                        ),
                    )
                }
                13u16 => {
                    Ok(
                        Which::Disembargo(
                            <_p::Struct<
                                super::Disembargo,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Message::DISEMBARGO.field,
                            ),
                        ),
                    )
                }
                unknown => Err(_p::NotInSchema(unknown)),
            }
        }
    }
}
#[derive(Clone)]
pub struct Bootstrap<T = _p::Family>(T);
impl _p::ty::SchemaType for Bootstrap {
    const ID: u64 = 16811039658553601732u64;
}
impl<T> _p::IntoFamily for Bootstrap<T> {
    type Family = Bootstrap;
}
impl<T: _p::Capable> _p::Capable for Bootstrap<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Bootstrap<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Bootstrap(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for bootstrap::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for bootstrap::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Bootstrap(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<bootstrap::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: bootstrap::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for bootstrap::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for bootstrap::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for bootstrap::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<bootstrap::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: bootstrap::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for bootstrap::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for bootstrap::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for bootstrap::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Bootstrap {
    type Reader<'a, T: _p::rpc::Table> = bootstrap::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = bootstrap::Builder<'a, T>;
}
impl _p::ty::Struct for Bootstrap {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 1u16,
    };
}
impl Bootstrap {
    const QUESTION_ID: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 0u32,
        default: 0u32,
    };
    const DEPRECATED_OBJECT_ID: _p::Descriptor<_p::AnyPtr> = _p::Descriptor::<
        _p::AnyPtr,
    > {
        slot: 0u32,
        default: ::core::option::Option::None,
    };
}
impl<'p, T: _p::rpc::Table + 'p> bootstrap::Reader<'p, T> {
    #[inline]
    pub fn question_id(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(&self.0, &Bootstrap::QUESTION_ID)
        }
    }
    #[inline]
    pub fn deprecated_object_id(&self) -> _p::Accessor<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(
                &self.0,
                &Bootstrap::DEPRECATED_OBJECT_ID,
            )
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> bootstrap::Builder<'p, T> {
    #[inline]
    pub fn question_id(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(&mut self.0, &Bootstrap::QUESTION_ID)
        }
    }
    #[inline]
    pub fn deprecated_object_id(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(
                &mut self.0,
                &Bootstrap::DEPRECATED_OBJECT_ID,
            )
        }
    }
    #[inline]
    pub fn into_deprecated_object_id(self) -> _p::AccessorOwned<'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(
                self.0,
                &Bootstrap::DEPRECATED_OBJECT_ID,
            )
        }
    }
}
pub mod bootstrap {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Bootstrap<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Bootstrap<
        _p::StructBuilder<'a, T>,
    >;
}
#[derive(Clone)]
pub struct Call<T = _p::Family>(T);
impl _p::ty::SchemaType for Call {
    const ID: u64 = 9469473312751832276u64;
}
impl<T> _p::IntoFamily for Call<T> {
    type Family = Call;
}
impl<T: _p::Capable> _p::Capable for Call<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Call<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Call(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for call::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for call::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Call(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<call::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: call::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for call::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for call::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for call::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<call::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: call::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for call::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for call::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for call::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Call {
    type Reader<'a, T: _p::rpc::Table> = call::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = call::Builder<'a, T>;
}
impl _p::ty::Struct for Call {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 3u16,
        ptrs: 3u16,
    };
}
impl Call {
    const QUESTION_ID: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 0u32,
        default: 0u32,
    };
    const TARGET: _p::Descriptor<_p::Struct<MessageTarget>> = _p::Descriptor::<
        _p::Struct<MessageTarget>,
    > {
        slot: 0u32,
        default: ::core::option::Option::None,
    };
    const INTERFACE_ID: _p::Descriptor<u64> = _p::Descriptor::<u64> {
        slot: 1u32,
        default: 0u64,
    };
    const METHOD_ID: _p::Descriptor<u16> = _p::Descriptor::<u16> {
        slot: 2u32,
        default: 0u16,
    };
    const PARAMS: _p::Descriptor<_p::Struct<Payload>> = _p::Descriptor::<
        _p::Struct<Payload>,
    > {
        slot: 1u32,
        default: ::core::option::Option::None,
    };
    const SEND_RESULTS_TO: _p::Descriptor<_p::Group<call::SendResultsTo>> = ();
    const ALLOW_THIRD_PARTY_TAIL_CALL: _p::Descriptor<bool> = _p::Descriptor::<bool> {
        slot: 128u32,
        default: false,
    };
    const NO_PROMISE_PIPELINING: _p::Descriptor<bool> = _p::Descriptor::<bool> {
        slot: 129u32,
        default: false,
    };
    const ONLY_PROMISE_PIPELINE: _p::Descriptor<bool> = _p::Descriptor::<bool> {
        slot: 130u32,
        default: false,
    };
}
impl<'p, T: _p::rpc::Table + 'p> call::Reader<'p, T> {
    #[inline]
    pub fn question_id(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe { <u32 as _p::field::FieldType>::accessor(&self.0, &Call::QUESTION_ID) }
    }
    #[inline]
    pub fn target(&self) -> _p::Accessor<'_, 'p, T, _p::Struct<MessageTarget>> {
        unsafe {
            <_p::Struct<
                MessageTarget,
            > as _p::field::FieldType>::accessor(&self.0, &Call::TARGET)
        }
    }
    #[inline]
    pub fn interface_id(&self) -> _p::Accessor<'_, 'p, T, u64> {
        unsafe { <u64 as _p::field::FieldType>::accessor(&self.0, &Call::INTERFACE_ID) }
    }
    #[inline]
    pub fn method_id(&self) -> _p::Accessor<'_, 'p, T, u16> {
        unsafe { <u16 as _p::field::FieldType>::accessor(&self.0, &Call::METHOD_ID) }
    }
    #[inline]
    pub fn params(&self) -> _p::Accessor<'_, 'p, T, _p::Struct<Payload>> {
        unsafe {
            <_p::Struct<
                Payload,
            > as _p::field::FieldType>::accessor(&self.0, &Call::PARAMS)
        }
    }
    #[inline]
    pub fn send_results_to(
        &self,
    ) -> _p::Accessor<'_, 'p, T, _p::Group<call::SendResultsTo>> {
        unsafe {
            <_p::Group<
                call::SendResultsTo,
            > as _p::field::FieldType>::accessor(&self.0, &Call::SEND_RESULTS_TO)
        }
    }
    #[inline]
    pub fn allow_third_party_tail_call(&self) -> _p::Accessor<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &self.0,
                &Call::ALLOW_THIRD_PARTY_TAIL_CALL,
            )
        }
    }
    #[inline]
    pub fn no_promise_pipelining(&self) -> _p::Accessor<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &self.0,
                &Call::NO_PROMISE_PIPELINING,
            )
        }
    }
    #[inline]
    pub fn only_promise_pipeline(&self) -> _p::Accessor<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &self.0,
                &Call::ONLY_PROMISE_PIPELINE,
            )
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> call::Builder<'p, T> {
    #[inline]
    pub fn question_id(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(&mut self.0, &Call::QUESTION_ID)
        }
    }
    #[inline]
    pub fn target(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::Struct<MessageTarget>> {
        unsafe {
            <_p::Struct<
                MessageTarget,
            > as _p::field::FieldType>::accessor(&mut self.0, &Call::TARGET)
        }
    }
    #[inline]
    pub fn interface_id(&mut self) -> _p::AccessorMut<'_, 'p, T, u64> {
        unsafe {
            <u64 as _p::field::FieldType>::accessor(&mut self.0, &Call::INTERFACE_ID)
        }
    }
    #[inline]
    pub fn method_id(&mut self) -> _p::AccessorMut<'_, 'p, T, u16> {
        unsafe { <u16 as _p::field::FieldType>::accessor(&mut self.0, &Call::METHOD_ID) }
    }
    #[inline]
    pub fn params(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::Struct<Payload>> {
        unsafe {
            <_p::Struct<
                Payload,
            > as _p::field::FieldType>::accessor(&mut self.0, &Call::PARAMS)
        }
    }
    #[inline]
    pub fn send_results_to(
        &mut self,
    ) -> _p::AccessorMut<'_, 'p, T, _p::Group<call::SendResultsTo>> {
        unsafe {
            <_p::Group<
                call::SendResultsTo,
            > as _p::field::FieldType>::accessor(&mut self.0, &Call::SEND_RESULTS_TO)
        }
    }
    #[inline]
    pub fn allow_third_party_tail_call(&mut self) -> _p::AccessorMut<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &mut self.0,
                &Call::ALLOW_THIRD_PARTY_TAIL_CALL,
            )
        }
    }
    #[inline]
    pub fn no_promise_pipelining(&mut self) -> _p::AccessorMut<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &mut self.0,
                &Call::NO_PROMISE_PIPELINING,
            )
        }
    }
    #[inline]
    pub fn only_promise_pipeline(&mut self) -> _p::AccessorMut<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &mut self.0,
                &Call::ONLY_PROMISE_PIPELINE,
            )
        }
    }
    #[inline]
    pub fn into_target(self) -> _p::AccessorOwned<'p, T, _p::Struct<MessageTarget>> {
        unsafe {
            <_p::Struct<
                MessageTarget,
            > as _p::field::FieldType>::accessor(self.0, &Call::TARGET)
        }
    }
    #[inline]
    pub fn into_params(self) -> _p::AccessorOwned<'p, T, _p::Struct<Payload>> {
        unsafe {
            <_p::Struct<
                Payload,
            > as _p::field::FieldType>::accessor(self.0, &Call::PARAMS)
        }
    }
    #[inline]
    pub fn into_send_results_to(
        self,
    ) -> _p::AccessorOwned<'p, T, _p::Group<call::SendResultsTo>> {
        unsafe {
            <_p::Group<
                call::SendResultsTo,
            > as _p::field::FieldType>::accessor(self.0, &Call::SEND_RESULTS_TO)
        }
    }
}
pub mod call {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Call<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Call<_p::StructBuilder<'a, T>>;
    #[derive(Clone)]
    pub struct SendResultsTo<T = _p::Family>(T);
    impl _p::ty::SchemaType for SendResultsTo {
        const ID: u64 = 15774052265921044377u64;
    }
    impl<T> _p::IntoFamily for SendResultsTo<T> {
        type Family = SendResultsTo;
    }
    impl<T: _p::Capable> _p::Capable for SendResultsTo<T> {
        type Table = T::Table;
        type Imbued = T::Imbued;
        type ImbuedWith<T2: _p::rpc::Table> = SendResultsTo<T::ImbuedWith<T2>>;
        #[inline]
        fn imbued(&self) -> &Self::Imbued {
            self.0.imbued()
        }
        #[inline]
        fn imbue_release<T2: _p::rpc::Table>(
            self,
            new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
        ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
            let (imbued, old) = self.0.imbue_release(new_table);
            (SendResultsTo(imbued), old)
        }
        #[inline]
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
    impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for send_results_to::Reader<'a, T> {
        type Ptr = _p::StructReader<'a, T>;
    }
    impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
    for send_results_to::Reader<'a, T> {
        #[inline]
        fn from(ptr: _p::StructReader<'a, T>) -> Self {
            SendResultsTo(ptr)
        }
    }
    impl<'a, T: _p::rpc::Table> core::convert::From<send_results_to::Reader<'a, T>>
    for _p::StructReader<'a, T> {
        #[inline]
        fn from(reader: send_results_to::Reader<'a, T>) -> Self {
            reader.0
        }
    }
    impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
    for send_results_to::Reader<'a, T> {
        #[inline]
        fn as_ref(&self) -> &_p::StructReader<'a, T> {
            &self.0
        }
    }
    impl<'a, T: _p::rpc::Table> _p::ty::StructReader for send_results_to::Reader<'a, T> {}
    impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for send_results_to::Builder<'a, T> {
        type Ptr = _p::StructBuilder<'a, T>;
    }
    impl<'a, T: _p::rpc::Table> core::convert::From<send_results_to::Builder<'a, T>>
    for _p::StructBuilder<'a, T> {
        #[inline]
        fn from(reader: send_results_to::Builder<'a, T>) -> Self {
            reader.0
        }
    }
    impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
    for send_results_to::Builder<'a, T> {
        #[inline]
        fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
            &self.0
        }
    }
    impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
    for send_results_to::Builder<'a, T> {
        #[inline]
        fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
            &mut self.0
        }
    }
    impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder
    for send_results_to::Builder<'a, T> {
        unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
            Self(ptr)
        }
    }
    impl _p::StructView for SendResultsTo {
        type Reader<'a, T: _p::rpc::Table> = send_results_to::Reader<'a, T>;
        type Builder<'a, T: _p::rpc::Table> = send_results_to::Builder<'a, T>;
    }
    impl _p::FieldGroup for SendResultsTo {
        unsafe fn clear<'a, 'b, T: _p::rpc::Table>(s: &'a mut _p::StructBuilder<'b, T>) {
            s.set_field_unchecked(3usize, 0);
            <() as _p::field::FieldType>::clear(s, &SendResultsTo::CALLER.field);
        }
    }
    impl SendResultsTo {
        const CALLER: _p::VariantDescriptor<()> = _p::VariantDescriptor::<()> {
            variant: _p::VariantInfo {
                slot: 3u32,
                case: 0u16,
            },
            field: (),
        };
        const YOURSELF: _p::VariantDescriptor<()> = _p::VariantDescriptor::<()> {
            variant: _p::VariantInfo {
                slot: 3u32,
                case: 1u16,
            },
            field: (),
        };
        const THIRD_PARTY: _p::VariantDescriptor<_p::AnyPtr> = _p::VariantDescriptor::<
            _p::AnyPtr,
        > {
            variant: _p::VariantInfo {
                slot: 3u32,
                case: 2u16,
            },
            field: _p::Descriptor::<_p::AnyPtr> {
                slot: 2u32,
                default: ::core::option::Option::None,
            },
        };
    }
    impl<'p, T: _p::rpc::Table + 'p> send_results_to::Reader<'p, T> {
        #[inline]
        pub fn caller(&self) -> _p::Variant<'_, 'p, T, ()> {
            unsafe {
                <() as _p::field::FieldType>::variant(&self.0, &SendResultsTo::CALLER)
            }
        }
        #[inline]
        pub fn yourself(&self) -> _p::Variant<'_, 'p, T, ()> {
            unsafe {
                <() as _p::field::FieldType>::variant(&self.0, &SendResultsTo::YOURSELF)
            }
        }
        #[inline]
        pub fn third_party(&self) -> _p::Variant<'_, 'p, T, _p::AnyPtr> {
            unsafe {
                <_p::AnyPtr as _p::field::FieldType>::variant(
                    &self.0,
                    &SendResultsTo::THIRD_PARTY,
                )
            }
        }
        #[inline]
        pub fn which(&self) -> Result<send_results_to::Which<&Self>, _p::NotInSchema> {
            unsafe { <send_results_to::Which<_> as _p::UnionViewer<_>>::get(self) }
        }
    }
    impl<'p, T: _p::rpc::Table + 'p> send_results_to::Builder<'p, T> {
        #[inline]
        pub fn caller(&mut self) -> _p::VariantMut<'_, 'p, T, ()> {
            unsafe {
                <() as _p::field::FieldType>::variant(
                    &mut self.0,
                    &SendResultsTo::CALLER,
                )
            }
        }
        #[inline]
        pub fn yourself(&mut self) -> _p::VariantMut<'_, 'p, T, ()> {
            unsafe {
                <() as _p::field::FieldType>::variant(
                    &mut self.0,
                    &SendResultsTo::YOURSELF,
                )
            }
        }
        #[inline]
        pub fn third_party(&mut self) -> _p::VariantMut<'_, 'p, T, _p::AnyPtr> {
            unsafe {
                <_p::AnyPtr as _p::field::FieldType>::variant(
                    &mut self.0,
                    &SendResultsTo::THIRD_PARTY,
                )
            }
        }
        #[inline]
        pub fn into_third_party(self) -> _p::VariantOwned<'p, T, _p::AnyPtr> {
            unsafe {
                <_p::AnyPtr as _p::field::FieldType>::variant(
                    self.0,
                    &SendResultsTo::THIRD_PARTY,
                )
            }
        }
        #[inline]
        pub fn which(
            &mut self,
        ) -> Result<send_results_to::Which<&mut Self>, _p::NotInSchema> {
            unsafe { <send_results_to::Which<_> as _p::UnionViewer<_>>::get(self) }
        }
    }
    pub mod send_results_to {
        use super::{__file, __imports, _p};
        pub type Reader<'a, T = _p::rpc::Empty> = super::SendResultsTo<
            _p::StructReader<'a, T>,
        >;
        pub type Builder<'a, T = _p::rpc::Empty> = super::SendResultsTo<
            _p::StructBuilder<'a, T>,
        >;
        pub enum Which<T: _p::Viewable = _p::Family> {
            Caller(_p::ViewOf<T, ()>),
            Yourself(_p::ViewOf<T, ()>),
            ThirdParty(_p::ViewOf<T, _p::AnyPtr>),
        }
        impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b Reader<'p, T>>
        for Which {
            type View = Which<&'b Reader<'p, T>>;
            unsafe fn get(
                repr: &'b Reader<'p, T>,
            ) -> Result<Self::View, _p::NotInSchema> {
                let tag = repr.0.data_field::<u16>(3usize);
                match tag {
                    0u16 => {
                        Ok(
                            Which::Caller(
                                <() as _p::field::FieldType>::accessor(
                                    &repr.0,
                                    &super::SendResultsTo::CALLER.field,
                                ),
                            ),
                        )
                    }
                    1u16 => {
                        Ok(
                            Which::Yourself(
                                <() as _p::field::FieldType>::accessor(
                                    &repr.0,
                                    &super::SendResultsTo::YOURSELF.field,
                                ),
                            ),
                        )
                    }
                    2u16 => {
                        Ok(
                            Which::ThirdParty(
                                <_p::AnyPtr as _p::field::FieldType>::accessor(
                                    &repr.0,
                                    &super::SendResultsTo::THIRD_PARTY.field,
                                ),
                            ),
                        )
                    }
                    unknown => Err(_p::NotInSchema(unknown)),
                }
            }
        }
        impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b mut Builder<'p, T>>
        for Which {
            type View = Which<&'b mut Builder<'p, T>>;
            unsafe fn get(
                repr: &'b mut Builder<'p, T>,
            ) -> Result<Self::View, _p::NotInSchema> {
                let tag = repr.0.data_field::<u16>(3usize);
                match tag {
                    0u16 => {
                        Ok(
                            Which::Caller(
                                <() as _p::field::FieldType>::accessor(
                                    &mut repr.0,
                                    &super::SendResultsTo::CALLER.field,
                                ),
                            ),
                        )
                    }
                    1u16 => {
                        Ok(
                            Which::Yourself(
                                <() as _p::field::FieldType>::accessor(
                                    &mut repr.0,
                                    &super::SendResultsTo::YOURSELF.field,
                                ),
                            ),
                        )
                    }
                    2u16 => {
                        Ok(
                            Which::ThirdParty(
                                <_p::AnyPtr as _p::field::FieldType>::accessor(
                                    &mut repr.0,
                                    &super::SendResultsTo::THIRD_PARTY.field,
                                ),
                            ),
                        )
                    }
                    unknown => Err(_p::NotInSchema(unknown)),
                }
            }
        }
    }
}
#[derive(Clone)]
pub struct Return<T = _p::Family>(T);
impl _p::ty::SchemaType for Return {
    const ID: u64 = 11392333052105676602u64;
}
impl<T> _p::IntoFamily for Return<T> {
    type Family = Return;
}
impl<T: _p::Capable> _p::Capable for Return<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Return<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Return(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for r#return::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for r#return::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Return(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<r#return::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: r#return::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for r#return::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for r#return::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for r#return::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<r#return::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: r#return::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for r#return::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for r#return::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for r#return::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Return {
    type Reader<'a, T: _p::rpc::Table> = r#return::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = r#return::Builder<'a, T>;
}
impl _p::ty::Struct for Return {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 2u16,
        ptrs: 1u16,
    };
}
impl Return {
    const ANSWER_ID: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 0u32,
        default: 0u32,
    };
    const RELEASE_PARAM_CAPS: _p::Descriptor<bool> = _p::Descriptor::<bool> {
        slot: 32u32,
        default: true,
    };
    const NO_FINISH_NEEDED: _p::Descriptor<bool> = _p::Descriptor::<bool> {
        slot: 33u32,
        default: false,
    };
    const RESULTS: _p::VariantDescriptor<_p::Struct<Payload>> = _p::VariantDescriptor::<
        _p::Struct<Payload>,
    > {
        variant: _p::VariantInfo {
            slot: 3u32,
            case: 0u16,
        },
        field: _p::Descriptor::<_p::Struct<Payload>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const EXCEPTION: _p::VariantDescriptor<_p::Struct<Exception>> = _p::VariantDescriptor::<
        _p::Struct<Exception>,
    > {
        variant: _p::VariantInfo {
            slot: 3u32,
            case: 1u16,
        },
        field: _p::Descriptor::<_p::Struct<Exception>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const CANCELED: _p::VariantDescriptor<()> = _p::VariantDescriptor::<()> {
        variant: _p::VariantInfo {
            slot: 3u32,
            case: 2u16,
        },
        field: (),
    };
    const RESULTS_SENT_ELSEWHERE: _p::VariantDescriptor<()> = _p::VariantDescriptor::<
        (),
    > {
        variant: _p::VariantInfo {
            slot: 3u32,
            case: 3u16,
        },
        field: (),
    };
    const TAKE_FROM_OTHER_QUESTION: _p::VariantDescriptor<u32> = _p::VariantDescriptor::<
        u32,
    > {
        variant: _p::VariantInfo {
            slot: 3u32,
            case: 4u16,
        },
        field: _p::Descriptor::<u32> {
            slot: 2u32,
            default: 0u32,
        },
    };
    const ACCEPT_FROM_THIRD_PARTY: _p::VariantDescriptor<_p::AnyPtr> = _p::VariantDescriptor::<
        _p::AnyPtr,
    > {
        variant: _p::VariantInfo {
            slot: 3u32,
            case: 5u16,
        },
        field: _p::Descriptor::<_p::AnyPtr> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
}
impl<'p, T: _p::rpc::Table + 'p> r#return::Reader<'p, T> {
    #[inline]
    pub fn answer_id(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe { <u32 as _p::field::FieldType>::accessor(&self.0, &Return::ANSWER_ID) }
    }
    #[inline]
    pub fn release_param_caps(&self) -> _p::Accessor<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &self.0,
                &Return::RELEASE_PARAM_CAPS,
            )
        }
    }
    #[inline]
    pub fn no_finish_needed(&self) -> _p::Accessor<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(&self.0, &Return::NO_FINISH_NEEDED)
        }
    }
    #[inline]
    pub fn results(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Payload>> {
        unsafe {
            <_p::Struct<
                Payload,
            > as _p::field::FieldType>::variant(&self.0, &Return::RESULTS)
        }
    }
    #[inline]
    pub fn exception(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Exception>> {
        unsafe {
            <_p::Struct<
                Exception,
            > as _p::field::FieldType>::variant(&self.0, &Return::EXCEPTION)
        }
    }
    #[inline]
    pub fn canceled(&self) -> _p::Variant<'_, 'p, T, ()> {
        unsafe { <() as _p::field::FieldType>::variant(&self.0, &Return::CANCELED) }
    }
    #[inline]
    pub fn results_sent_elsewhere(&self) -> _p::Variant<'_, 'p, T, ()> {
        unsafe {
            <() as _p::field::FieldType>::variant(
                &self.0,
                &Return::RESULTS_SENT_ELSEWHERE,
            )
        }
    }
    #[inline]
    pub fn take_from_other_question(&self) -> _p::Variant<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::variant(
                &self.0,
                &Return::TAKE_FROM_OTHER_QUESTION,
            )
        }
    }
    #[inline]
    pub fn accept_from_third_party(&self) -> _p::Variant<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::variant(
                &self.0,
                &Return::ACCEPT_FROM_THIRD_PARTY,
            )
        }
    }
    #[inline]
    pub fn which(&self) -> Result<r#return::Which<&Self>, _p::NotInSchema> {
        unsafe { <r#return::Which<_> as _p::UnionViewer<_>>::get(self) }
    }
}
impl<'p, T: _p::rpc::Table + 'p> r#return::Builder<'p, T> {
    #[inline]
    pub fn answer_id(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(&mut self.0, &Return::ANSWER_ID)
        }
    }
    #[inline]
    pub fn release_param_caps(&mut self) -> _p::AccessorMut<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &mut self.0,
                &Return::RELEASE_PARAM_CAPS,
            )
        }
    }
    #[inline]
    pub fn no_finish_needed(&mut self) -> _p::AccessorMut<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &mut self.0,
                &Return::NO_FINISH_NEEDED,
            )
        }
    }
    #[inline]
    pub fn results(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Payload>> {
        unsafe {
            <_p::Struct<
                Payload,
            > as _p::field::FieldType>::variant(&mut self.0, &Return::RESULTS)
        }
    }
    #[inline]
    pub fn exception(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Exception>> {
        unsafe {
            <_p::Struct<
                Exception,
            > as _p::field::FieldType>::variant(&mut self.0, &Return::EXCEPTION)
        }
    }
    #[inline]
    pub fn canceled(&mut self) -> _p::VariantMut<'_, 'p, T, ()> {
        unsafe { <() as _p::field::FieldType>::variant(&mut self.0, &Return::CANCELED) }
    }
    #[inline]
    pub fn results_sent_elsewhere(&mut self) -> _p::VariantMut<'_, 'p, T, ()> {
        unsafe {
            <() as _p::field::FieldType>::variant(
                &mut self.0,
                &Return::RESULTS_SENT_ELSEWHERE,
            )
        }
    }
    #[inline]
    pub fn take_from_other_question(&mut self) -> _p::VariantMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::variant(
                &mut self.0,
                &Return::TAKE_FROM_OTHER_QUESTION,
            )
        }
    }
    #[inline]
    pub fn accept_from_third_party(&mut self) -> _p::VariantMut<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::variant(
                &mut self.0,
                &Return::ACCEPT_FROM_THIRD_PARTY,
            )
        }
    }
    #[inline]
    pub fn into_results(self) -> _p::VariantOwned<'p, T, _p::Struct<Payload>> {
        unsafe {
            <_p::Struct<
                Payload,
            > as _p::field::FieldType>::variant(self.0, &Return::RESULTS)
        }
    }
    #[inline]
    pub fn into_exception(self) -> _p::VariantOwned<'p, T, _p::Struct<Exception>> {
        unsafe {
            <_p::Struct<
                Exception,
            > as _p::field::FieldType>::variant(self.0, &Return::EXCEPTION)
        }
    }
    #[inline]
    pub fn into_accept_from_third_party(self) -> _p::VariantOwned<'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::variant(
                self.0,
                &Return::ACCEPT_FROM_THIRD_PARTY,
            )
        }
    }
    #[inline]
    pub fn which(&mut self) -> Result<r#return::Which<&mut Self>, _p::NotInSchema> {
        unsafe { <r#return::Which<_> as _p::UnionViewer<_>>::get(self) }
    }
}
pub mod r#return {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Return<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Return<_p::StructBuilder<'a, T>>;
    pub enum Which<T: _p::Viewable = _p::Family> {
        Results(_p::ViewOf<T, _p::Struct<super::Payload>>),
        Exception(_p::ViewOf<T, _p::Struct<super::Exception>>),
        Canceled(_p::ViewOf<T, ()>),
        ResultsSentElsewhere(_p::ViewOf<T, ()>),
        TakeFromOtherQuestion(_p::ViewOf<T, u32>),
        AcceptFromThirdParty(_p::ViewOf<T, _p::AnyPtr>),
    }
    impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b Reader<'p, T>> for Which {
        type View = Which<&'b Reader<'p, T>>;
        unsafe fn get(repr: &'b Reader<'p, T>) -> Result<Self::View, _p::NotInSchema> {
            let tag = repr.0.data_field::<u16>(3usize);
            match tag {
                0u16 => {
                    Ok(
                        Which::Results(
                            <_p::Struct<
                                super::Payload,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Return::RESULTS.field,
                            ),
                        ),
                    )
                }
                1u16 => {
                    Ok(
                        Which::Exception(
                            <_p::Struct<
                                super::Exception,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Return::EXCEPTION.field,
                            ),
                        ),
                    )
                }
                2u16 => {
                    Ok(
                        Which::Canceled(
                            <() as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Return::CANCELED.field,
                            ),
                        ),
                    )
                }
                3u16 => {
                    Ok(
                        Which::ResultsSentElsewhere(
                            <() as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Return::RESULTS_SENT_ELSEWHERE.field,
                            ),
                        ),
                    )
                }
                4u16 => {
                    Ok(
                        Which::TakeFromOtherQuestion(
                            <u32 as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Return::TAKE_FROM_OTHER_QUESTION.field,
                            ),
                        ),
                    )
                }
                5u16 => {
                    Ok(
                        Which::AcceptFromThirdParty(
                            <_p::AnyPtr as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Return::ACCEPT_FROM_THIRD_PARTY.field,
                            ),
                        ),
                    )
                }
                unknown => Err(_p::NotInSchema(unknown)),
            }
        }
    }
    impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b mut Builder<'p, T>>
    for Which {
        type View = Which<&'b mut Builder<'p, T>>;
        unsafe fn get(
            repr: &'b mut Builder<'p, T>,
        ) -> Result<Self::View, _p::NotInSchema> {
            let tag = repr.0.data_field::<u16>(3usize);
            match tag {
                0u16 => {
                    Ok(
                        Which::Results(
                            <_p::Struct<
                                super::Payload,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Return::RESULTS.field,
                            ),
                        ),
                    )
                }
                1u16 => {
                    Ok(
                        Which::Exception(
                            <_p::Struct<
                                super::Exception,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Return::EXCEPTION.field,
                            ),
                        ),
                    )
                }
                2u16 => {
                    Ok(
                        Which::Canceled(
                            <() as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Return::CANCELED.field,
                            ),
                        ),
                    )
                }
                3u16 => {
                    Ok(
                        Which::ResultsSentElsewhere(
                            <() as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Return::RESULTS_SENT_ELSEWHERE.field,
                            ),
                        ),
                    )
                }
                4u16 => {
                    Ok(
                        Which::TakeFromOtherQuestion(
                            <u32 as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Return::TAKE_FROM_OTHER_QUESTION.field,
                            ),
                        ),
                    )
                }
                5u16 => {
                    Ok(
                        Which::AcceptFromThirdParty(
                            <_p::AnyPtr as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Return::ACCEPT_FROM_THIRD_PARTY.field,
                            ),
                        ),
                    )
                }
                unknown => Err(_p::NotInSchema(unknown)),
            }
        }
    }
}
#[derive(Clone)]
pub struct Finish<T = _p::Family>(T);
impl _p::ty::SchemaType for Finish {
    const ID: u64 = 15239388059401719395u64;
}
impl<T> _p::IntoFamily for Finish<T> {
    type Family = Finish;
}
impl<T: _p::Capable> _p::Capable for Finish<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Finish<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Finish(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for finish::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for finish::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Finish(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<finish::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: finish::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for finish::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for finish::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for finish::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<finish::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: finish::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for finish::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for finish::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for finish::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Finish {
    type Reader<'a, T: _p::rpc::Table> = finish::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = finish::Builder<'a, T>;
}
impl _p::ty::Struct for Finish {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 0u16,
    };
}
impl Finish {
    const QUESTION_ID: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 0u32,
        default: 0u32,
    };
    const RELEASE_RESULT_CAPS: _p::Descriptor<bool> = _p::Descriptor::<bool> {
        slot: 32u32,
        default: true,
    };
}
impl<'p, T: _p::rpc::Table + 'p> finish::Reader<'p, T> {
    #[inline]
    pub fn question_id(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe { <u32 as _p::field::FieldType>::accessor(&self.0, &Finish::QUESTION_ID) }
    }
    #[inline]
    pub fn release_result_caps(&self) -> _p::Accessor<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &self.0,
                &Finish::RELEASE_RESULT_CAPS,
            )
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> finish::Builder<'p, T> {
    #[inline]
    pub fn question_id(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(&mut self.0, &Finish::QUESTION_ID)
        }
    }
    #[inline]
    pub fn release_result_caps(&mut self) -> _p::AccessorMut<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &mut self.0,
                &Finish::RELEASE_RESULT_CAPS,
            )
        }
    }
}
pub mod finish {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Finish<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Finish<_p::StructBuilder<'a, T>>;
}
#[derive(Clone)]
pub struct Resolve<T = _p::Family>(T);
impl _p::ty::SchemaType for Resolve {
    const ID: u64 = 13529541526594062446u64;
}
impl<T> _p::IntoFamily for Resolve<T> {
    type Family = Resolve;
}
impl<T: _p::Capable> _p::Capable for Resolve<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Resolve<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Resolve(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for resolve::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for resolve::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Resolve(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<resolve::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: resolve::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for resolve::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for resolve::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for resolve::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<resolve::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: resolve::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for resolve::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for resolve::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for resolve::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Resolve {
    type Reader<'a, T: _p::rpc::Table> = resolve::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = resolve::Builder<'a, T>;
}
impl _p::ty::Struct for Resolve {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 1u16,
    };
}
impl Resolve {
    const PROMISE_ID: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 0u32,
        default: 0u32,
    };
    const CAP: _p::VariantDescriptor<_p::Struct<CapDescriptor>> = _p::VariantDescriptor::<
        _p::Struct<CapDescriptor>,
    > {
        variant: _p::VariantInfo {
            slot: 2u32,
            case: 0u16,
        },
        field: _p::Descriptor::<_p::Struct<CapDescriptor>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const EXCEPTION: _p::VariantDescriptor<_p::Struct<Exception>> = _p::VariantDescriptor::<
        _p::Struct<Exception>,
    > {
        variant: _p::VariantInfo {
            slot: 2u32,
            case: 1u16,
        },
        field: _p::Descriptor::<_p::Struct<Exception>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
}
impl<'p, T: _p::rpc::Table + 'p> resolve::Reader<'p, T> {
    #[inline]
    pub fn promise_id(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe { <u32 as _p::field::FieldType>::accessor(&self.0, &Resolve::PROMISE_ID) }
    }
    #[inline]
    pub fn cap(&self) -> _p::Variant<'_, 'p, T, _p::Struct<CapDescriptor>> {
        unsafe {
            <_p::Struct<
                CapDescriptor,
            > as _p::field::FieldType>::variant(&self.0, &Resolve::CAP)
        }
    }
    #[inline]
    pub fn exception(&self) -> _p::Variant<'_, 'p, T, _p::Struct<Exception>> {
        unsafe {
            <_p::Struct<
                Exception,
            > as _p::field::FieldType>::variant(&self.0, &Resolve::EXCEPTION)
        }
    }
    #[inline]
    pub fn which(&self) -> Result<resolve::Which<&Self>, _p::NotInSchema> {
        unsafe { <resolve::Which<_> as _p::UnionViewer<_>>::get(self) }
    }
}
impl<'p, T: _p::rpc::Table + 'p> resolve::Builder<'p, T> {
    #[inline]
    pub fn promise_id(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(&mut self.0, &Resolve::PROMISE_ID)
        }
    }
    #[inline]
    pub fn cap(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<CapDescriptor>> {
        unsafe {
            <_p::Struct<
                CapDescriptor,
            > as _p::field::FieldType>::variant(&mut self.0, &Resolve::CAP)
        }
    }
    #[inline]
    pub fn exception(&mut self) -> _p::VariantMut<'_, 'p, T, _p::Struct<Exception>> {
        unsafe {
            <_p::Struct<
                Exception,
            > as _p::field::FieldType>::variant(&mut self.0, &Resolve::EXCEPTION)
        }
    }
    #[inline]
    pub fn into_cap(self) -> _p::VariantOwned<'p, T, _p::Struct<CapDescriptor>> {
        unsafe {
            <_p::Struct<
                CapDescriptor,
            > as _p::field::FieldType>::variant(self.0, &Resolve::CAP)
        }
    }
    #[inline]
    pub fn into_exception(self) -> _p::VariantOwned<'p, T, _p::Struct<Exception>> {
        unsafe {
            <_p::Struct<
                Exception,
            > as _p::field::FieldType>::variant(self.0, &Resolve::EXCEPTION)
        }
    }
    #[inline]
    pub fn which(&mut self) -> Result<resolve::Which<&mut Self>, _p::NotInSchema> {
        unsafe { <resolve::Which<_> as _p::UnionViewer<_>>::get(self) }
    }
}
pub mod resolve {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Resolve<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Resolve<_p::StructBuilder<'a, T>>;
    pub enum Which<T: _p::Viewable = _p::Family> {
        Cap(_p::ViewOf<T, _p::Struct<super::CapDescriptor>>),
        Exception(_p::ViewOf<T, _p::Struct<super::Exception>>),
    }
    impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b Reader<'p, T>> for Which {
        type View = Which<&'b Reader<'p, T>>;
        unsafe fn get(repr: &'b Reader<'p, T>) -> Result<Self::View, _p::NotInSchema> {
            let tag = repr.0.data_field::<u16>(2usize);
            match tag {
                0u16 => {
                    Ok(
                        Which::Cap(
                            <_p::Struct<
                                super::CapDescriptor,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Resolve::CAP.field,
                            ),
                        ),
                    )
                }
                1u16 => {
                    Ok(
                        Which::Exception(
                            <_p::Struct<
                                super::Exception,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::Resolve::EXCEPTION.field,
                            ),
                        ),
                    )
                }
                unknown => Err(_p::NotInSchema(unknown)),
            }
        }
    }
    impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b mut Builder<'p, T>>
    for Which {
        type View = Which<&'b mut Builder<'p, T>>;
        unsafe fn get(
            repr: &'b mut Builder<'p, T>,
        ) -> Result<Self::View, _p::NotInSchema> {
            let tag = repr.0.data_field::<u16>(2usize);
            match tag {
                0u16 => {
                    Ok(
                        Which::Cap(
                            <_p::Struct<
                                super::CapDescriptor,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Resolve::CAP.field,
                            ),
                        ),
                    )
                }
                1u16 => {
                    Ok(
                        Which::Exception(
                            <_p::Struct<
                                super::Exception,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::Resolve::EXCEPTION.field,
                            ),
                        ),
                    )
                }
                unknown => Err(_p::NotInSchema(unknown)),
            }
        }
    }
}
#[derive(Clone)]
pub struct Release<T = _p::Family>(T);
impl _p::ty::SchemaType for Release {
    const ID: u64 = 12473400923157197975u64;
}
impl<T> _p::IntoFamily for Release<T> {
    type Family = Release;
}
impl<T: _p::Capable> _p::Capable for Release<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Release<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Release(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for release::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for release::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Release(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<release::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: release::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for release::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for release::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for release::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<release::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: release::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for release::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for release::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for release::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Release {
    type Reader<'a, T: _p::rpc::Table> = release::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = release::Builder<'a, T>;
}
impl _p::ty::Struct for Release {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 0u16,
    };
}
impl Release {
    const ID: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 0u32,
        default: 0u32,
    };
    const REFERENCE_COUNT: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 1u32,
        default: 0u32,
    };
}
impl<'p, T: _p::rpc::Table + 'p> release::Reader<'p, T> {
    #[inline]
    pub fn id(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe { <u32 as _p::field::FieldType>::accessor(&self.0, &Release::ID) }
    }
    #[inline]
    pub fn reference_count(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(&self.0, &Release::REFERENCE_COUNT)
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> release::Builder<'p, T> {
    #[inline]
    pub fn id(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe { <u32 as _p::field::FieldType>::accessor(&mut self.0, &Release::ID) }
    }
    #[inline]
    pub fn reference_count(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(
                &mut self.0,
                &Release::REFERENCE_COUNT,
            )
        }
    }
}
pub mod release {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Release<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Release<_p::StructBuilder<'a, T>>;
}
#[derive(Clone)]
pub struct Disembargo<T = _p::Family>(T);
impl _p::ty::SchemaType for Disembargo {
    const ID: u64 = 17970548384007534353u64;
}
impl<T> _p::IntoFamily for Disembargo<T> {
    type Family = Disembargo;
}
impl<T: _p::Capable> _p::Capable for Disembargo<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Disembargo<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Disembargo(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for disembargo::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for disembargo::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Disembargo(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<disembargo::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: disembargo::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for disembargo::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for disembargo::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for disembargo::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<disembargo::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: disembargo::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for disembargo::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for disembargo::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for disembargo::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Disembargo {
    type Reader<'a, T: _p::rpc::Table> = disembargo::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = disembargo::Builder<'a, T>;
}
impl _p::ty::Struct for Disembargo {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 1u16,
    };
}
impl Disembargo {
    const TARGET: _p::Descriptor<_p::Struct<MessageTarget>> = _p::Descriptor::<
        _p::Struct<MessageTarget>,
    > {
        slot: 0u32,
        default: ::core::option::Option::None,
    };
    const CONTEXT: _p::Descriptor<_p::Group<disembargo::Context>> = ();
}
impl<'p, T: _p::rpc::Table + 'p> disembargo::Reader<'p, T> {
    #[inline]
    pub fn target(&self) -> _p::Accessor<'_, 'p, T, _p::Struct<MessageTarget>> {
        unsafe {
            <_p::Struct<
                MessageTarget,
            > as _p::field::FieldType>::accessor(&self.0, &Disembargo::TARGET)
        }
    }
    #[inline]
    pub fn context(&self) -> _p::Accessor<'_, 'p, T, _p::Group<disembargo::Context>> {
        unsafe {
            <_p::Group<
                disembargo::Context,
            > as _p::field::FieldType>::accessor(&self.0, &Disembargo::CONTEXT)
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> disembargo::Builder<'p, T> {
    #[inline]
    pub fn target(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::Struct<MessageTarget>> {
        unsafe {
            <_p::Struct<
                MessageTarget,
            > as _p::field::FieldType>::accessor(&mut self.0, &Disembargo::TARGET)
        }
    }
    #[inline]
    pub fn context(
        &mut self,
    ) -> _p::AccessorMut<'_, 'p, T, _p::Group<disembargo::Context>> {
        unsafe {
            <_p::Group<
                disembargo::Context,
            > as _p::field::FieldType>::accessor(&mut self.0, &Disembargo::CONTEXT)
        }
    }
    #[inline]
    pub fn into_target(self) -> _p::AccessorOwned<'p, T, _p::Struct<MessageTarget>> {
        unsafe {
            <_p::Struct<
                MessageTarget,
            > as _p::field::FieldType>::accessor(self.0, &Disembargo::TARGET)
        }
    }
    #[inline]
    pub fn into_context(
        self,
    ) -> _p::AccessorOwned<'p, T, _p::Group<disembargo::Context>> {
        unsafe {
            <_p::Group<
                disembargo::Context,
            > as _p::field::FieldType>::accessor(self.0, &Disembargo::CONTEXT)
        }
    }
}
pub mod disembargo {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Disembargo<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Disembargo<
        _p::StructBuilder<'a, T>,
    >;
    #[derive(Clone)]
    pub struct Context<T = _p::Family>(T);
    impl _p::ty::SchemaType for Context {
        const ID: u64 = 15376050949367520589u64;
    }
    impl<T> _p::IntoFamily for Context<T> {
        type Family = Context;
    }
    impl<T: _p::Capable> _p::Capable for Context<T> {
        type Table = T::Table;
        type Imbued = T::Imbued;
        type ImbuedWith<T2: _p::rpc::Table> = Context<T::ImbuedWith<T2>>;
        #[inline]
        fn imbued(&self) -> &Self::Imbued {
            self.0.imbued()
        }
        #[inline]
        fn imbue_release<T2: _p::rpc::Table>(
            self,
            new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
        ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
            let (imbued, old) = self.0.imbue_release(new_table);
            (Context(imbued), old)
        }
        #[inline]
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
    impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for context::Reader<'a, T> {
        type Ptr = _p::StructReader<'a, T>;
    }
    impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
    for context::Reader<'a, T> {
        #[inline]
        fn from(ptr: _p::StructReader<'a, T>) -> Self {
            Context(ptr)
        }
    }
    impl<'a, T: _p::rpc::Table> core::convert::From<context::Reader<'a, T>>
    for _p::StructReader<'a, T> {
        #[inline]
        fn from(reader: context::Reader<'a, T>) -> Self {
            reader.0
        }
    }
    impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
    for context::Reader<'a, T> {
        #[inline]
        fn as_ref(&self) -> &_p::StructReader<'a, T> {
            &self.0
        }
    }
    impl<'a, T: _p::rpc::Table> _p::ty::StructReader for context::Reader<'a, T> {}
    impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for context::Builder<'a, T> {
        type Ptr = _p::StructBuilder<'a, T>;
    }
    impl<'a, T: _p::rpc::Table> core::convert::From<context::Builder<'a, T>>
    for _p::StructBuilder<'a, T> {
        #[inline]
        fn from(reader: context::Builder<'a, T>) -> Self {
            reader.0
        }
    }
    impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
    for context::Builder<'a, T> {
        #[inline]
        fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
            &self.0
        }
    }
    impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
    for context::Builder<'a, T> {
        #[inline]
        fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
            &mut self.0
        }
    }
    impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for context::Builder<'a, T> {
        unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
            Self(ptr)
        }
    }
    impl _p::StructView for Context {
        type Reader<'a, T: _p::rpc::Table> = context::Reader<'a, T>;
        type Builder<'a, T: _p::rpc::Table> = context::Builder<'a, T>;
    }
    impl _p::FieldGroup for Context {
        unsafe fn clear<'a, 'b, T: _p::rpc::Table>(s: &'a mut _p::StructBuilder<'b, T>) {
            s.set_field_unchecked(2usize, 0);
            <u32 as _p::field::FieldType>::clear(s, &Context::SENDER_LOOPBACK.field);
        }
    }
    impl Context {
        const SENDER_LOOPBACK: _p::VariantDescriptor<u32> = _p::VariantDescriptor::<
            u32,
        > {
            variant: _p::VariantInfo {
                slot: 2u32,
                case: 0u16,
            },
            field: _p::Descriptor::<u32> {
                slot: 0u32,
                default: 0u32,
            },
        };
        const RECEIVER_LOOPBACK: _p::VariantDescriptor<u32> = _p::VariantDescriptor::<
            u32,
        > {
            variant: _p::VariantInfo {
                slot: 2u32,
                case: 1u16,
            },
            field: _p::Descriptor::<u32> {
                slot: 0u32,
                default: 0u32,
            },
        };
        const ACCEPT: _p::VariantDescriptor<()> = _p::VariantDescriptor::<()> {
            variant: _p::VariantInfo {
                slot: 2u32,
                case: 2u16,
            },
            field: (),
        };
        const PROVIDE: _p::VariantDescriptor<u32> = _p::VariantDescriptor::<u32> {
            variant: _p::VariantInfo {
                slot: 2u32,
                case: 3u16,
            },
            field: _p::Descriptor::<u32> {
                slot: 0u32,
                default: 0u32,
            },
        };
    }
    impl<'p, T: _p::rpc::Table + 'p> context::Reader<'p, T> {
        #[inline]
        pub fn sender_loopback(&self) -> _p::Variant<'_, 'p, T, u32> {
            unsafe {
                <u32 as _p::field::FieldType>::variant(
                    &self.0,
                    &Context::SENDER_LOOPBACK,
                )
            }
        }
        #[inline]
        pub fn receiver_loopback(&self) -> _p::Variant<'_, 'p, T, u32> {
            unsafe {
                <u32 as _p::field::FieldType>::variant(
                    &self.0,
                    &Context::RECEIVER_LOOPBACK,
                )
            }
        }
        #[inline]
        pub fn accept(&self) -> _p::Variant<'_, 'p, T, ()> {
            unsafe { <() as _p::field::FieldType>::variant(&self.0, &Context::ACCEPT) }
        }
        #[inline]
        pub fn provide(&self) -> _p::Variant<'_, 'p, T, u32> {
            unsafe { <u32 as _p::field::FieldType>::variant(&self.0, &Context::PROVIDE) }
        }
        #[inline]
        pub fn which(&self) -> Result<context::Which<&Self>, _p::NotInSchema> {
            unsafe { <context::Which<_> as _p::UnionViewer<_>>::get(self) }
        }
    }
    impl<'p, T: _p::rpc::Table + 'p> context::Builder<'p, T> {
        #[inline]
        pub fn sender_loopback(&mut self) -> _p::VariantMut<'_, 'p, T, u32> {
            unsafe {
                <u32 as _p::field::FieldType>::variant(
                    &mut self.0,
                    &Context::SENDER_LOOPBACK,
                )
            }
        }
        #[inline]
        pub fn receiver_loopback(&mut self) -> _p::VariantMut<'_, 'p, T, u32> {
            unsafe {
                <u32 as _p::field::FieldType>::variant(
                    &mut self.0,
                    &Context::RECEIVER_LOOPBACK,
                )
            }
        }
        #[inline]
        pub fn accept(&mut self) -> _p::VariantMut<'_, 'p, T, ()> {
            unsafe {
                <() as _p::field::FieldType>::variant(&mut self.0, &Context::ACCEPT)
            }
        }
        #[inline]
        pub fn provide(&mut self) -> _p::VariantMut<'_, 'p, T, u32> {
            unsafe {
                <u32 as _p::field::FieldType>::variant(&mut self.0, &Context::PROVIDE)
            }
        }
        #[inline]
        pub fn which(&mut self) -> Result<context::Which<&mut Self>, _p::NotInSchema> {
            unsafe { <context::Which<_> as _p::UnionViewer<_>>::get(self) }
        }
    }
    pub mod context {
        use super::{__file, __imports, _p};
        pub type Reader<'a, T = _p::rpc::Empty> = super::Context<
            _p::StructReader<'a, T>,
        >;
        pub type Builder<'a, T = _p::rpc::Empty> = super::Context<
            _p::StructBuilder<'a, T>,
        >;
        pub enum Which<T: _p::Viewable = _p::Family> {
            SenderLoopback(_p::ViewOf<T, u32>),
            ReceiverLoopback(_p::ViewOf<T, u32>),
            Accept(_p::ViewOf<T, ()>),
            Provide(_p::ViewOf<T, u32>),
        }
        impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b Reader<'p, T>>
        for Which {
            type View = Which<&'b Reader<'p, T>>;
            unsafe fn get(
                repr: &'b Reader<'p, T>,
            ) -> Result<Self::View, _p::NotInSchema> {
                let tag = repr.0.data_field::<u16>(2usize);
                match tag {
                    0u16 => {
                        Ok(
                            Which::SenderLoopback(
                                <u32 as _p::field::FieldType>::accessor(
                                    &repr.0,
                                    &super::Context::SENDER_LOOPBACK.field,
                                ),
                            ),
                        )
                    }
                    1u16 => {
                        Ok(
                            Which::ReceiverLoopback(
                                <u32 as _p::field::FieldType>::accessor(
                                    &repr.0,
                                    &super::Context::RECEIVER_LOOPBACK.field,
                                ),
                            ),
                        )
                    }
                    2u16 => {
                        Ok(
                            Which::Accept(
                                <() as _p::field::FieldType>::accessor(
                                    &repr.0,
                                    &super::Context::ACCEPT.field,
                                ),
                            ),
                        )
                    }
                    3u16 => {
                        Ok(
                            Which::Provide(
                                <u32 as _p::field::FieldType>::accessor(
                                    &repr.0,
                                    &super::Context::PROVIDE.field,
                                ),
                            ),
                        )
                    }
                    unknown => Err(_p::NotInSchema(unknown)),
                }
            }
        }
        impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b mut Builder<'p, T>>
        for Which {
            type View = Which<&'b mut Builder<'p, T>>;
            unsafe fn get(
                repr: &'b mut Builder<'p, T>,
            ) -> Result<Self::View, _p::NotInSchema> {
                let tag = repr.0.data_field::<u16>(2usize);
                match tag {
                    0u16 => {
                        Ok(
                            Which::SenderLoopback(
                                <u32 as _p::field::FieldType>::accessor(
                                    &mut repr.0,
                                    &super::Context::SENDER_LOOPBACK.field,
                                ),
                            ),
                        )
                    }
                    1u16 => {
                        Ok(
                            Which::ReceiverLoopback(
                                <u32 as _p::field::FieldType>::accessor(
                                    &mut repr.0,
                                    &super::Context::RECEIVER_LOOPBACK.field,
                                ),
                            ),
                        )
                    }
                    2u16 => {
                        Ok(
                            Which::Accept(
                                <() as _p::field::FieldType>::accessor(
                                    &mut repr.0,
                                    &super::Context::ACCEPT.field,
                                ),
                            ),
                        )
                    }
                    3u16 => {
                        Ok(
                            Which::Provide(
                                <u32 as _p::field::FieldType>::accessor(
                                    &mut repr.0,
                                    &super::Context::PROVIDE.field,
                                ),
                            ),
                        )
                    }
                    unknown => Err(_p::NotInSchema(unknown)),
                }
            }
        }
    }
}
#[derive(Clone)]
pub struct Provide<T = _p::Family>(T);
impl _p::ty::SchemaType for Provide {
    const ID: u64 = 11270825879279873114u64;
}
impl<T> _p::IntoFamily for Provide<T> {
    type Family = Provide;
}
impl<T: _p::Capable> _p::Capable for Provide<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Provide<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Provide(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for provide::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for provide::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Provide(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<provide::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: provide::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for provide::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for provide::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for provide::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<provide::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: provide::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for provide::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for provide::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for provide::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Provide {
    type Reader<'a, T: _p::rpc::Table> = provide::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = provide::Builder<'a, T>;
}
impl _p::ty::Struct for Provide {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 2u16,
    };
}
impl Provide {
    const QUESTION_ID: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 0u32,
        default: 0u32,
    };
    const TARGET: _p::Descriptor<_p::Struct<MessageTarget>> = _p::Descriptor::<
        _p::Struct<MessageTarget>,
    > {
        slot: 0u32,
        default: ::core::option::Option::None,
    };
    const RECIPIENT: _p::Descriptor<_p::AnyPtr> = _p::Descriptor::<_p::AnyPtr> {
        slot: 1u32,
        default: ::core::option::Option::None,
    };
}
impl<'p, T: _p::rpc::Table + 'p> provide::Reader<'p, T> {
    #[inline]
    pub fn question_id(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(&self.0, &Provide::QUESTION_ID)
        }
    }
    #[inline]
    pub fn target(&self) -> _p::Accessor<'_, 'p, T, _p::Struct<MessageTarget>> {
        unsafe {
            <_p::Struct<
                MessageTarget,
            > as _p::field::FieldType>::accessor(&self.0, &Provide::TARGET)
        }
    }
    #[inline]
    pub fn recipient(&self) -> _p::Accessor<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(&self.0, &Provide::RECIPIENT)
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> provide::Builder<'p, T> {
    #[inline]
    pub fn question_id(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(&mut self.0, &Provide::QUESTION_ID)
        }
    }
    #[inline]
    pub fn target(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::Struct<MessageTarget>> {
        unsafe {
            <_p::Struct<
                MessageTarget,
            > as _p::field::FieldType>::accessor(&mut self.0, &Provide::TARGET)
        }
    }
    #[inline]
    pub fn recipient(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(
                &mut self.0,
                &Provide::RECIPIENT,
            )
        }
    }
    #[inline]
    pub fn into_target(self) -> _p::AccessorOwned<'p, T, _p::Struct<MessageTarget>> {
        unsafe {
            <_p::Struct<
                MessageTarget,
            > as _p::field::FieldType>::accessor(self.0, &Provide::TARGET)
        }
    }
    #[inline]
    pub fn into_recipient(self) -> _p::AccessorOwned<'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(self.0, &Provide::RECIPIENT)
        }
    }
}
pub mod provide {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Provide<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Provide<_p::StructBuilder<'a, T>>;
}
#[derive(Clone)]
pub struct Accept<T = _p::Family>(T);
impl _p::ty::SchemaType for Accept {
    const ID: u64 = 15332985841292492822u64;
}
impl<T> _p::IntoFamily for Accept<T> {
    type Family = Accept;
}
impl<T: _p::Capable> _p::Capable for Accept<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Accept<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Accept(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for accept::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for accept::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Accept(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<accept::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: accept::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for accept::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for accept::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for accept::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<accept::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: accept::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for accept::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for accept::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for accept::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Accept {
    type Reader<'a, T: _p::rpc::Table> = accept::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = accept::Builder<'a, T>;
}
impl _p::ty::Struct for Accept {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 1u16,
    };
}
impl Accept {
    const QUESTION_ID: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 0u32,
        default: 0u32,
    };
    const PROVISION: _p::Descriptor<_p::AnyPtr> = _p::Descriptor::<_p::AnyPtr> {
        slot: 0u32,
        default: ::core::option::Option::None,
    };
    const EMBARGO: _p::Descriptor<bool> = _p::Descriptor::<bool> {
        slot: 32u32,
        default: false,
    };
}
impl<'p, T: _p::rpc::Table + 'p> accept::Reader<'p, T> {
    #[inline]
    pub fn question_id(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe { <u32 as _p::field::FieldType>::accessor(&self.0, &Accept::QUESTION_ID) }
    }
    #[inline]
    pub fn provision(&self) -> _p::Accessor<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(&self.0, &Accept::PROVISION)
        }
    }
    #[inline]
    pub fn embargo(&self) -> _p::Accessor<'_, 'p, T, bool> {
        unsafe { <bool as _p::field::FieldType>::accessor(&self.0, &Accept::EMBARGO) }
    }
}
impl<'p, T: _p::rpc::Table + 'p> accept::Builder<'p, T> {
    #[inline]
    pub fn question_id(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(&mut self.0, &Accept::QUESTION_ID)
        }
    }
    #[inline]
    pub fn provision(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(
                &mut self.0,
                &Accept::PROVISION,
            )
        }
    }
    #[inline]
    pub fn embargo(&mut self) -> _p::AccessorMut<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(&mut self.0, &Accept::EMBARGO)
        }
    }
    #[inline]
    pub fn into_provision(self) -> _p::AccessorOwned<'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(self.0, &Accept::PROVISION)
        }
    }
}
pub mod accept {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Accept<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Accept<_p::StructBuilder<'a, T>>;
}
#[derive(Clone)]
pub struct Join<T = _p::Family>(T);
impl _p::ty::SchemaType for Join {
    const ID: u64 = 18149955118657700271u64;
}
impl<T> _p::IntoFamily for Join<T> {
    type Family = Join;
}
impl<T: _p::Capable> _p::Capable for Join<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Join<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Join(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for join::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for join::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Join(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<join::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: join::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for join::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for join::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for join::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<join::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: join::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for join::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for join::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for join::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Join {
    type Reader<'a, T: _p::rpc::Table> = join::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = join::Builder<'a, T>;
}
impl _p::ty::Struct for Join {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 2u16,
    };
}
impl Join {
    const QUESTION_ID: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 0u32,
        default: 0u32,
    };
    const TARGET: _p::Descriptor<_p::Struct<MessageTarget>> = _p::Descriptor::<
        _p::Struct<MessageTarget>,
    > {
        slot: 0u32,
        default: ::core::option::Option::None,
    };
    const KEY_PART: _p::Descriptor<_p::AnyPtr> = _p::Descriptor::<_p::AnyPtr> {
        slot: 1u32,
        default: ::core::option::Option::None,
    };
}
impl<'p, T: _p::rpc::Table + 'p> join::Reader<'p, T> {
    #[inline]
    pub fn question_id(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe { <u32 as _p::field::FieldType>::accessor(&self.0, &Join::QUESTION_ID) }
    }
    #[inline]
    pub fn target(&self) -> _p::Accessor<'_, 'p, T, _p::Struct<MessageTarget>> {
        unsafe {
            <_p::Struct<
                MessageTarget,
            > as _p::field::FieldType>::accessor(&self.0, &Join::TARGET)
        }
    }
    #[inline]
    pub fn key_part(&self) -> _p::Accessor<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(&self.0, &Join::KEY_PART)
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> join::Builder<'p, T> {
    #[inline]
    pub fn question_id(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(&mut self.0, &Join::QUESTION_ID)
        }
    }
    #[inline]
    pub fn target(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::Struct<MessageTarget>> {
        unsafe {
            <_p::Struct<
                MessageTarget,
            > as _p::field::FieldType>::accessor(&mut self.0, &Join::TARGET)
        }
    }
    #[inline]
    pub fn key_part(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(&mut self.0, &Join::KEY_PART)
        }
    }
    #[inline]
    pub fn into_target(self) -> _p::AccessorOwned<'p, T, _p::Struct<MessageTarget>> {
        unsafe {
            <_p::Struct<
                MessageTarget,
            > as _p::field::FieldType>::accessor(self.0, &Join::TARGET)
        }
    }
    #[inline]
    pub fn into_key_part(self) -> _p::AccessorOwned<'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(self.0, &Join::KEY_PART)
        }
    }
}
pub mod join {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Join<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Join<_p::StructBuilder<'a, T>>;
}
#[derive(Clone)]
pub struct MessageTarget<T = _p::Family>(T);
impl _p::ty::SchemaType for MessageTarget {
    const ID: u64 = 10789521159760378817u64;
}
impl<T> _p::IntoFamily for MessageTarget<T> {
    type Family = MessageTarget;
}
impl<T: _p::Capable> _p::Capable for MessageTarget<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = MessageTarget<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (MessageTarget(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for message_target::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for message_target::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        MessageTarget(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<message_target::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: message_target::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for message_target::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for message_target::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for message_target::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<message_target::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: message_target::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for message_target::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for message_target::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for message_target::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for MessageTarget {
    type Reader<'a, T: _p::rpc::Table> = message_target::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = message_target::Builder<'a, T>;
}
impl _p::ty::Struct for MessageTarget {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 1u16,
    };
}
impl MessageTarget {
    const IMPORTED_CAP: _p::VariantDescriptor<u32> = _p::VariantDescriptor::<u32> {
        variant: _p::VariantInfo {
            slot: 2u32,
            case: 0u16,
        },
        field: _p::Descriptor::<u32> {
            slot: 0u32,
            default: 0u32,
        },
    };
    const PROMISED_ANSWER: _p::VariantDescriptor<_p::Struct<PromisedAnswer>> = _p::VariantDescriptor::<
        _p::Struct<PromisedAnswer>,
    > {
        variant: _p::VariantInfo {
            slot: 2u32,
            case: 1u16,
        },
        field: _p::Descriptor::<_p::Struct<PromisedAnswer>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
}
impl<'p, T: _p::rpc::Table + 'p> message_target::Reader<'p, T> {
    #[inline]
    pub fn imported_cap(&self) -> _p::Variant<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::variant(&self.0, &MessageTarget::IMPORTED_CAP)
        }
    }
    #[inline]
    pub fn promised_answer(&self) -> _p::Variant<'_, 'p, T, _p::Struct<PromisedAnswer>> {
        unsafe {
            <_p::Struct<
                PromisedAnswer,
            > as _p::field::FieldType>::variant(&self.0, &MessageTarget::PROMISED_ANSWER)
        }
    }
    #[inline]
    pub fn which(&self) -> Result<message_target::Which<&Self>, _p::NotInSchema> {
        unsafe { <message_target::Which<_> as _p::UnionViewer<_>>::get(self) }
    }
}
impl<'p, T: _p::rpc::Table + 'p> message_target::Builder<'p, T> {
    #[inline]
    pub fn imported_cap(&mut self) -> _p::VariantMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::variant(
                &mut self.0,
                &MessageTarget::IMPORTED_CAP,
            )
        }
    }
    #[inline]
    pub fn promised_answer(
        &mut self,
    ) -> _p::VariantMut<'_, 'p, T, _p::Struct<PromisedAnswer>> {
        unsafe {
            <_p::Struct<
                PromisedAnswer,
            > as _p::field::FieldType>::variant(
                &mut self.0,
                &MessageTarget::PROMISED_ANSWER,
            )
        }
    }
    #[inline]
    pub fn into_promised_answer(
        self,
    ) -> _p::VariantOwned<'p, T, _p::Struct<PromisedAnswer>> {
        unsafe {
            <_p::Struct<
                PromisedAnswer,
            > as _p::field::FieldType>::variant(self.0, &MessageTarget::PROMISED_ANSWER)
        }
    }
    #[inline]
    pub fn which(
        &mut self,
    ) -> Result<message_target::Which<&mut Self>, _p::NotInSchema> {
        unsafe { <message_target::Which<_> as _p::UnionViewer<_>>::get(self) }
    }
}
pub mod message_target {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::MessageTarget<
        _p::StructReader<'a, T>,
    >;
    pub type Builder<'a, T = _p::rpc::Empty> = super::MessageTarget<
        _p::StructBuilder<'a, T>,
    >;
    pub enum Which<T: _p::Viewable = _p::Family> {
        ImportedCap(_p::ViewOf<T, u32>),
        PromisedAnswer(_p::ViewOf<T, _p::Struct<super::PromisedAnswer>>),
    }
    impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b Reader<'p, T>> for Which {
        type View = Which<&'b Reader<'p, T>>;
        unsafe fn get(repr: &'b Reader<'p, T>) -> Result<Self::View, _p::NotInSchema> {
            let tag = repr.0.data_field::<u16>(2usize);
            match tag {
                0u16 => {
                    Ok(
                        Which::ImportedCap(
                            <u32 as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::MessageTarget::IMPORTED_CAP.field,
                            ),
                        ),
                    )
                }
                1u16 => {
                    Ok(
                        Which::PromisedAnswer(
                            <_p::Struct<
                                super::PromisedAnswer,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::MessageTarget::PROMISED_ANSWER.field,
                            ),
                        ),
                    )
                }
                unknown => Err(_p::NotInSchema(unknown)),
            }
        }
    }
    impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b mut Builder<'p, T>>
    for Which {
        type View = Which<&'b mut Builder<'p, T>>;
        unsafe fn get(
            repr: &'b mut Builder<'p, T>,
        ) -> Result<Self::View, _p::NotInSchema> {
            let tag = repr.0.data_field::<u16>(2usize);
            match tag {
                0u16 => {
                    Ok(
                        Which::ImportedCap(
                            <u32 as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::MessageTarget::IMPORTED_CAP.field,
                            ),
                        ),
                    )
                }
                1u16 => {
                    Ok(
                        Which::PromisedAnswer(
                            <_p::Struct<
                                super::PromisedAnswer,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::MessageTarget::PROMISED_ANSWER.field,
                            ),
                        ),
                    )
                }
                unknown => Err(_p::NotInSchema(unknown)),
            }
        }
    }
}
#[derive(Clone)]
pub struct Payload<T = _p::Family>(T);
impl _p::ty::SchemaType for Payload {
    const ID: u64 = 11100916931204903995u64;
}
impl<T> _p::IntoFamily for Payload<T> {
    type Family = Payload;
}
impl<T: _p::Capable> _p::Capable for Payload<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Payload<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Payload(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for payload::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for payload::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Payload(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<payload::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: payload::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for payload::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for payload::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for payload::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<payload::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: payload::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for payload::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for payload::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for payload::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Payload {
    type Reader<'a, T: _p::rpc::Table> = payload::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = payload::Builder<'a, T>;
}
impl _p::ty::Struct for Payload {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 0u16,
        ptrs: 2u16,
    };
}
impl Payload {
    const CONTENT: _p::Descriptor<_p::AnyPtr> = _p::Descriptor::<_p::AnyPtr> {
        slot: 0u32,
        default: ::core::option::Option::None,
    };
    const CAP_TABLE: _p::Descriptor<_p::List<_p::Struct<CapDescriptor>>> = _p::Descriptor::<
        _p::List<_p::Struct<CapDescriptor>>,
    > {
        slot: 1u32,
        default: ::core::option::Option::None,
    };
}
impl<'p, T: _p::rpc::Table + 'p> payload::Reader<'p, T> {
    #[inline]
    pub fn content(&self) -> _p::Accessor<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(&self.0, &Payload::CONTENT)
        }
    }
    #[inline]
    pub fn cap_table(
        &self,
    ) -> _p::Accessor<'_, 'p, T, _p::List<_p::Struct<CapDescriptor>>> {
        unsafe {
            <_p::List<
                _p::Struct<CapDescriptor>,
            > as _p::field::FieldType>::accessor(&self.0, &Payload::CAP_TABLE)
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> payload::Builder<'p, T> {
    #[inline]
    pub fn content(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(
                &mut self.0,
                &Payload::CONTENT,
            )
        }
    }
    #[inline]
    pub fn cap_table(
        &mut self,
    ) -> _p::AccessorMut<'_, 'p, T, _p::List<_p::Struct<CapDescriptor>>> {
        unsafe {
            <_p::List<
                _p::Struct<CapDescriptor>,
            > as _p::field::FieldType>::accessor(&mut self.0, &Payload::CAP_TABLE)
        }
    }
    #[inline]
    pub fn into_content(self) -> _p::AccessorOwned<'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(self.0, &Payload::CONTENT)
        }
    }
    #[inline]
    pub fn into_cap_table(
        self,
    ) -> _p::AccessorOwned<'p, T, _p::List<_p::Struct<CapDescriptor>>> {
        unsafe {
            <_p::List<
                _p::Struct<CapDescriptor>,
            > as _p::field::FieldType>::accessor(self.0, &Payload::CAP_TABLE)
        }
    }
}
pub mod payload {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Payload<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Payload<_p::StructBuilder<'a, T>>;
}
#[derive(Clone)]
pub struct CapDescriptor<T = _p::Family>(T);
impl _p::ty::SchemaType for CapDescriptor {
    const ID: u64 = 9593755465305995440u64;
}
impl<T> _p::IntoFamily for CapDescriptor<T> {
    type Family = CapDescriptor;
}
impl<T: _p::Capable> _p::Capable for CapDescriptor<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = CapDescriptor<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (CapDescriptor(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for cap_descriptor::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for cap_descriptor::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        CapDescriptor(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<cap_descriptor::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: cap_descriptor::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for cap_descriptor::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for cap_descriptor::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for cap_descriptor::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<cap_descriptor::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: cap_descriptor::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for cap_descriptor::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for cap_descriptor::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for cap_descriptor::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for CapDescriptor {
    type Reader<'a, T: _p::rpc::Table> = cap_descriptor::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = cap_descriptor::Builder<'a, T>;
}
impl _p::ty::Struct for CapDescriptor {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 1u16,
    };
}
impl CapDescriptor {
    const ATTACHED_FD: _p::Descriptor<u8> = _p::Descriptor::<u8> {
        slot: 2u32,
        default: 255u8,
    };
    const NONE: _p::VariantDescriptor<()> = _p::VariantDescriptor::<()> {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 0u16,
        },
        field: (),
    };
    const SENDER_HOSTED: _p::VariantDescriptor<u32> = _p::VariantDescriptor::<u32> {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 1u16,
        },
        field: _p::Descriptor::<u32> {
            slot: 1u32,
            default: 0u32,
        },
    };
    const SENDER_PROMISE: _p::VariantDescriptor<u32> = _p::VariantDescriptor::<u32> {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 2u16,
        },
        field: _p::Descriptor::<u32> {
            slot: 1u32,
            default: 0u32,
        },
    };
    const RECEIVER_HOSTED: _p::VariantDescriptor<u32> = _p::VariantDescriptor::<u32> {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 3u16,
        },
        field: _p::Descriptor::<u32> {
            slot: 1u32,
            default: 0u32,
        },
    };
    const RECEIVER_ANSWER: _p::VariantDescriptor<_p::Struct<PromisedAnswer>> = _p::VariantDescriptor::<
        _p::Struct<PromisedAnswer>,
    > {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 4u16,
        },
        field: _p::Descriptor::<_p::Struct<PromisedAnswer>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
    const THIRD_PARTY_HOSTED: _p::VariantDescriptor<
        _p::Struct<ThirdPartyCapDescriptor>,
    > = _p::VariantDescriptor::<_p::Struct<ThirdPartyCapDescriptor>> {
        variant: _p::VariantInfo {
            slot: 0u32,
            case: 5u16,
        },
        field: _p::Descriptor::<_p::Struct<ThirdPartyCapDescriptor>> {
            slot: 0u32,
            default: ::core::option::Option::None,
        },
    };
}
impl<'p, T: _p::rpc::Table + 'p> cap_descriptor::Reader<'p, T> {
    #[inline]
    pub fn attached_fd(&self) -> _p::Accessor<'_, 'p, T, u8> {
        unsafe {
            <u8 as _p::field::FieldType>::accessor(&self.0, &CapDescriptor::ATTACHED_FD)
        }
    }
    #[inline]
    pub fn none(&self) -> _p::Variant<'_, 'p, T, ()> {
        unsafe { <() as _p::field::FieldType>::variant(&self.0, &CapDescriptor::NONE) }
    }
    #[inline]
    pub fn sender_hosted(&self) -> _p::Variant<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::variant(
                &self.0,
                &CapDescriptor::SENDER_HOSTED,
            )
        }
    }
    #[inline]
    pub fn sender_promise(&self) -> _p::Variant<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::variant(
                &self.0,
                &CapDescriptor::SENDER_PROMISE,
            )
        }
    }
    #[inline]
    pub fn receiver_hosted(&self) -> _p::Variant<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::variant(
                &self.0,
                &CapDescriptor::RECEIVER_HOSTED,
            )
        }
    }
    #[inline]
    pub fn receiver_answer(&self) -> _p::Variant<'_, 'p, T, _p::Struct<PromisedAnswer>> {
        unsafe {
            <_p::Struct<
                PromisedAnswer,
            > as _p::field::FieldType>::variant(&self.0, &CapDescriptor::RECEIVER_ANSWER)
        }
    }
    #[inline]
    pub fn third_party_hosted(
        &self,
    ) -> _p::Variant<'_, 'p, T, _p::Struct<ThirdPartyCapDescriptor>> {
        unsafe {
            <_p::Struct<
                ThirdPartyCapDescriptor,
            > as _p::field::FieldType>::variant(
                &self.0,
                &CapDescriptor::THIRD_PARTY_HOSTED,
            )
        }
    }
    #[inline]
    pub fn which(&self) -> Result<cap_descriptor::Which<&Self>, _p::NotInSchema> {
        unsafe { <cap_descriptor::Which<_> as _p::UnionViewer<_>>::get(self) }
    }
}
impl<'p, T: _p::rpc::Table + 'p> cap_descriptor::Builder<'p, T> {
    #[inline]
    pub fn attached_fd(&mut self) -> _p::AccessorMut<'_, 'p, T, u8> {
        unsafe {
            <u8 as _p::field::FieldType>::accessor(
                &mut self.0,
                &CapDescriptor::ATTACHED_FD,
            )
        }
    }
    #[inline]
    pub fn none(&mut self) -> _p::VariantMut<'_, 'p, T, ()> {
        unsafe {
            <() as _p::field::FieldType>::variant(&mut self.0, &CapDescriptor::NONE)
        }
    }
    #[inline]
    pub fn sender_hosted(&mut self) -> _p::VariantMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::variant(
                &mut self.0,
                &CapDescriptor::SENDER_HOSTED,
            )
        }
    }
    #[inline]
    pub fn sender_promise(&mut self) -> _p::VariantMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::variant(
                &mut self.0,
                &CapDescriptor::SENDER_PROMISE,
            )
        }
    }
    #[inline]
    pub fn receiver_hosted(&mut self) -> _p::VariantMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::variant(
                &mut self.0,
                &CapDescriptor::RECEIVER_HOSTED,
            )
        }
    }
    #[inline]
    pub fn receiver_answer(
        &mut self,
    ) -> _p::VariantMut<'_, 'p, T, _p::Struct<PromisedAnswer>> {
        unsafe {
            <_p::Struct<
                PromisedAnswer,
            > as _p::field::FieldType>::variant(
                &mut self.0,
                &CapDescriptor::RECEIVER_ANSWER,
            )
        }
    }
    #[inline]
    pub fn third_party_hosted(
        &mut self,
    ) -> _p::VariantMut<'_, 'p, T, _p::Struct<ThirdPartyCapDescriptor>> {
        unsafe {
            <_p::Struct<
                ThirdPartyCapDescriptor,
            > as _p::field::FieldType>::variant(
                &mut self.0,
                &CapDescriptor::THIRD_PARTY_HOSTED,
            )
        }
    }
    #[inline]
    pub fn into_receiver_answer(
        self,
    ) -> _p::VariantOwned<'p, T, _p::Struct<PromisedAnswer>> {
        unsafe {
            <_p::Struct<
                PromisedAnswer,
            > as _p::field::FieldType>::variant(self.0, &CapDescriptor::RECEIVER_ANSWER)
        }
    }
    #[inline]
    pub fn into_third_party_hosted(
        self,
    ) -> _p::VariantOwned<'p, T, _p::Struct<ThirdPartyCapDescriptor>> {
        unsafe {
            <_p::Struct<
                ThirdPartyCapDescriptor,
            > as _p::field::FieldType>::variant(
                self.0,
                &CapDescriptor::THIRD_PARTY_HOSTED,
            )
        }
    }
    #[inline]
    pub fn which(
        &mut self,
    ) -> Result<cap_descriptor::Which<&mut Self>, _p::NotInSchema> {
        unsafe { <cap_descriptor::Which<_> as _p::UnionViewer<_>>::get(self) }
    }
}
pub mod cap_descriptor {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::CapDescriptor<
        _p::StructReader<'a, T>,
    >;
    pub type Builder<'a, T = _p::rpc::Empty> = super::CapDescriptor<
        _p::StructBuilder<'a, T>,
    >;
    pub enum Which<T: _p::Viewable = _p::Family> {
        None(_p::ViewOf<T, ()>),
        SenderHosted(_p::ViewOf<T, u32>),
        SenderPromise(_p::ViewOf<T, u32>),
        ReceiverHosted(_p::ViewOf<T, u32>),
        ReceiverAnswer(_p::ViewOf<T, _p::Struct<super::PromisedAnswer>>),
        ThirdPartyHosted(_p::ViewOf<T, _p::Struct<super::ThirdPartyCapDescriptor>>),
    }
    impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b Reader<'p, T>> for Which {
        type View = Which<&'b Reader<'p, T>>;
        unsafe fn get(repr: &'b Reader<'p, T>) -> Result<Self::View, _p::NotInSchema> {
            let tag = repr.0.data_field::<u16>(0usize);
            match tag {
                0u16 => {
                    Ok(
                        Which::None(
                            <() as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::CapDescriptor::NONE.field,
                            ),
                        ),
                    )
                }
                1u16 => {
                    Ok(
                        Which::SenderHosted(
                            <u32 as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::CapDescriptor::SENDER_HOSTED.field,
                            ),
                        ),
                    )
                }
                2u16 => {
                    Ok(
                        Which::SenderPromise(
                            <u32 as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::CapDescriptor::SENDER_PROMISE.field,
                            ),
                        ),
                    )
                }
                3u16 => {
                    Ok(
                        Which::ReceiverHosted(
                            <u32 as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::CapDescriptor::RECEIVER_HOSTED.field,
                            ),
                        ),
                    )
                }
                4u16 => {
                    Ok(
                        Which::ReceiverAnswer(
                            <_p::Struct<
                                super::PromisedAnswer,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::CapDescriptor::RECEIVER_ANSWER.field,
                            ),
                        ),
                    )
                }
                5u16 => {
                    Ok(
                        Which::ThirdPartyHosted(
                            <_p::Struct<
                                super::ThirdPartyCapDescriptor,
                            > as _p::field::FieldType>::accessor(
                                &repr.0,
                                &super::CapDescriptor::THIRD_PARTY_HOSTED.field,
                            ),
                        ),
                    )
                }
                unknown => Err(_p::NotInSchema(unknown)),
            }
        }
    }
    impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b mut Builder<'p, T>>
    for Which {
        type View = Which<&'b mut Builder<'p, T>>;
        unsafe fn get(
            repr: &'b mut Builder<'p, T>,
        ) -> Result<Self::View, _p::NotInSchema> {
            let tag = repr.0.data_field::<u16>(0usize);
            match tag {
                0u16 => {
                    Ok(
                        Which::None(
                            <() as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::CapDescriptor::NONE.field,
                            ),
                        ),
                    )
                }
                1u16 => {
                    Ok(
                        Which::SenderHosted(
                            <u32 as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::CapDescriptor::SENDER_HOSTED.field,
                            ),
                        ),
                    )
                }
                2u16 => {
                    Ok(
                        Which::SenderPromise(
                            <u32 as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::CapDescriptor::SENDER_PROMISE.field,
                            ),
                        ),
                    )
                }
                3u16 => {
                    Ok(
                        Which::ReceiverHosted(
                            <u32 as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::CapDescriptor::RECEIVER_HOSTED.field,
                            ),
                        ),
                    )
                }
                4u16 => {
                    Ok(
                        Which::ReceiverAnswer(
                            <_p::Struct<
                                super::PromisedAnswer,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::CapDescriptor::RECEIVER_ANSWER.field,
                            ),
                        ),
                    )
                }
                5u16 => {
                    Ok(
                        Which::ThirdPartyHosted(
                            <_p::Struct<
                                super::ThirdPartyCapDescriptor,
                            > as _p::field::FieldType>::accessor(
                                &mut repr.0,
                                &super::CapDescriptor::THIRD_PARTY_HOSTED.field,
                            ),
                        ),
                    )
                }
                unknown => Err(_p::NotInSchema(unknown)),
            }
        }
    }
}
#[derive(Clone)]
pub struct PromisedAnswer<T = _p::Family>(T);
impl _p::ty::SchemaType for PromisedAnswer {
    const ID: u64 = 15564635848320162976u64;
}
impl<T> _p::IntoFamily for PromisedAnswer<T> {
    type Family = PromisedAnswer;
}
impl<T: _p::Capable> _p::Capable for PromisedAnswer<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = PromisedAnswer<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (PromisedAnswer(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for promised_answer::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for promised_answer::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        PromisedAnswer(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<promised_answer::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: promised_answer::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for promised_answer::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for promised_answer::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for promised_answer::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<promised_answer::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: promised_answer::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for promised_answer::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for promised_answer::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for promised_answer::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for PromisedAnswer {
    type Reader<'a, T: _p::rpc::Table> = promised_answer::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = promised_answer::Builder<'a, T>;
}
impl _p::ty::Struct for PromisedAnswer {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 1u16,
    };
}
impl PromisedAnswer {
    const QUESTION_ID: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 0u32,
        default: 0u32,
    };
    const TRANSFORM: _p::Descriptor<_p::List<_p::Struct<promised_answer::Op>>> = _p::Descriptor::<
        _p::List<_p::Struct<promised_answer::Op>>,
    > {
        slot: 0u32,
        default: ::core::option::Option::None,
    };
}
impl<'p, T: _p::rpc::Table + 'p> promised_answer::Reader<'p, T> {
    #[inline]
    pub fn question_id(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(
                &self.0,
                &PromisedAnswer::QUESTION_ID,
            )
        }
    }
    #[inline]
    pub fn transform(
        &self,
    ) -> _p::Accessor<'_, 'p, T, _p::List<_p::Struct<promised_answer::Op>>> {
        unsafe {
            <_p::List<
                _p::Struct<promised_answer::Op>,
            > as _p::field::FieldType>::accessor(&self.0, &PromisedAnswer::TRANSFORM)
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> promised_answer::Builder<'p, T> {
    #[inline]
    pub fn question_id(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(
                &mut self.0,
                &PromisedAnswer::QUESTION_ID,
            )
        }
    }
    #[inline]
    pub fn transform(
        &mut self,
    ) -> _p::AccessorMut<'_, 'p, T, _p::List<_p::Struct<promised_answer::Op>>> {
        unsafe {
            <_p::List<
                _p::Struct<promised_answer::Op>,
            > as _p::field::FieldType>::accessor(&mut self.0, &PromisedAnswer::TRANSFORM)
        }
    }
    #[inline]
    pub fn into_transform(
        self,
    ) -> _p::AccessorOwned<'p, T, _p::List<_p::Struct<promised_answer::Op>>> {
        unsafe {
            <_p::List<
                _p::Struct<promised_answer::Op>,
            > as _p::field::FieldType>::accessor(self.0, &PromisedAnswer::TRANSFORM)
        }
    }
}
pub mod promised_answer {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::PromisedAnswer<
        _p::StructReader<'a, T>,
    >;
    pub type Builder<'a, T = _p::rpc::Empty> = super::PromisedAnswer<
        _p::StructBuilder<'a, T>,
    >;
    #[derive(Clone)]
    pub struct Op<T = _p::Family>(T);
    impl _p::ty::SchemaType for Op {
        const ID: u64 = 17516350820840804481u64;
    }
    impl<T> _p::IntoFamily for Op<T> {
        type Family = Op;
    }
    impl<T: _p::Capable> _p::Capable for Op<T> {
        type Table = T::Table;
        type Imbued = T::Imbued;
        type ImbuedWith<T2: _p::rpc::Table> = Op<T::ImbuedWith<T2>>;
        #[inline]
        fn imbued(&self) -> &Self::Imbued {
            self.0.imbued()
        }
        #[inline]
        fn imbue_release<T2: _p::rpc::Table>(
            self,
            new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
        ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
            let (imbued, old) = self.0.imbue_release(new_table);
            (Op(imbued), old)
        }
        #[inline]
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
    impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for op::Reader<'a, T> {
        type Ptr = _p::StructReader<'a, T>;
    }
    impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
    for op::Reader<'a, T> {
        #[inline]
        fn from(ptr: _p::StructReader<'a, T>) -> Self {
            Op(ptr)
        }
    }
    impl<'a, T: _p::rpc::Table> core::convert::From<op::Reader<'a, T>>
    for _p::StructReader<'a, T> {
        #[inline]
        fn from(reader: op::Reader<'a, T>) -> Self {
            reader.0
        }
    }
    impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
    for op::Reader<'a, T> {
        #[inline]
        fn as_ref(&self) -> &_p::StructReader<'a, T> {
            &self.0
        }
    }
    impl<'a, T: _p::rpc::Table> _p::ty::StructReader for op::Reader<'a, T> {}
    impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for op::Builder<'a, T> {
        type Ptr = _p::StructBuilder<'a, T>;
    }
    impl<'a, T: _p::rpc::Table> core::convert::From<op::Builder<'a, T>>
    for _p::StructBuilder<'a, T> {
        #[inline]
        fn from(reader: op::Builder<'a, T>) -> Self {
            reader.0
        }
    }
    impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
    for op::Builder<'a, T> {
        #[inline]
        fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
            &self.0
        }
    }
    impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
    for op::Builder<'a, T> {
        #[inline]
        fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
            &mut self.0
        }
    }
    impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for op::Builder<'a, T> {
        unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
            Self(ptr)
        }
    }
    impl _p::StructView for Op {
        type Reader<'a, T: _p::rpc::Table> = op::Reader<'a, T>;
        type Builder<'a, T: _p::rpc::Table> = op::Builder<'a, T>;
    }
    impl _p::ty::Struct for Op {
        const SIZE: _p::StructSize = _p::StructSize {
            data: 1u16,
            ptrs: 0u16,
        };
    }
    impl Op {
        const NOOP: _p::VariantDescriptor<()> = _p::VariantDescriptor::<()> {
            variant: _p::VariantInfo {
                slot: 0u32,
                case: 0u16,
            },
            field: (),
        };
        const GET_POINTER_FIELD: _p::VariantDescriptor<u16> = _p::VariantDescriptor::<
            u16,
        > {
            variant: _p::VariantInfo {
                slot: 0u32,
                case: 1u16,
            },
            field: _p::Descriptor::<u16> {
                slot: 1u32,
                default: 0u16,
            },
        };
    }
    impl<'p, T: _p::rpc::Table + 'p> op::Reader<'p, T> {
        #[inline]
        pub fn noop(&self) -> _p::Variant<'_, 'p, T, ()> {
            unsafe { <() as _p::field::FieldType>::variant(&self.0, &Op::NOOP) }
        }
        #[inline]
        pub fn get_pointer_field(&self) -> _p::Variant<'_, 'p, T, u16> {
            unsafe {
                <u16 as _p::field::FieldType>::variant(&self.0, &Op::GET_POINTER_FIELD)
            }
        }
        #[inline]
        pub fn which(&self) -> Result<op::Which<&Self>, _p::NotInSchema> {
            unsafe { <op::Which<_> as _p::UnionViewer<_>>::get(self) }
        }
    }
    impl<'p, T: _p::rpc::Table + 'p> op::Builder<'p, T> {
        #[inline]
        pub fn noop(&mut self) -> _p::VariantMut<'_, 'p, T, ()> {
            unsafe { <() as _p::field::FieldType>::variant(&mut self.0, &Op::NOOP) }
        }
        #[inline]
        pub fn get_pointer_field(&mut self) -> _p::VariantMut<'_, 'p, T, u16> {
            unsafe {
                <u16 as _p::field::FieldType>::variant(
                    &mut self.0,
                    &Op::GET_POINTER_FIELD,
                )
            }
        }
        #[inline]
        pub fn which(&mut self) -> Result<op::Which<&mut Self>, _p::NotInSchema> {
            unsafe { <op::Which<_> as _p::UnionViewer<_>>::get(self) }
        }
    }
    pub mod op {
        use super::{__file, __imports, _p};
        pub type Reader<'a, T = _p::rpc::Empty> = super::Op<_p::StructReader<'a, T>>;
        pub type Builder<'a, T = _p::rpc::Empty> = super::Op<_p::StructBuilder<'a, T>>;
        pub enum Which<T: _p::Viewable = _p::Family> {
            Noop(_p::ViewOf<T, ()>),
            GetPointerField(_p::ViewOf<T, u16>),
        }
        impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b Reader<'p, T>>
        for Which {
            type View = Which<&'b Reader<'p, T>>;
            unsafe fn get(
                repr: &'b Reader<'p, T>,
            ) -> Result<Self::View, _p::NotInSchema> {
                let tag = repr.0.data_field::<u16>(0usize);
                match tag {
                    0u16 => {
                        Ok(
                            Which::Noop(
                                <() as _p::field::FieldType>::accessor(
                                    &repr.0,
                                    &super::Op::NOOP.field,
                                ),
                            ),
                        )
                    }
                    1u16 => {
                        Ok(
                            Which::GetPointerField(
                                <u16 as _p::field::FieldType>::accessor(
                                    &repr.0,
                                    &super::Op::GET_POINTER_FIELD.field,
                                ),
                            ),
                        )
                    }
                    unknown => Err(_p::NotInSchema(unknown)),
                }
            }
        }
        impl<'b, 'p: 'b, T: _p::Table + 'p> _p::UnionViewer<&'b mut Builder<'p, T>>
        for Which {
            type View = Which<&'b mut Builder<'p, T>>;
            unsafe fn get(
                repr: &'b mut Builder<'p, T>,
            ) -> Result<Self::View, _p::NotInSchema> {
                let tag = repr.0.data_field::<u16>(0usize);
                match tag {
                    0u16 => {
                        Ok(
                            Which::Noop(
                                <() as _p::field::FieldType>::accessor(
                                    &mut repr.0,
                                    &super::Op::NOOP.field,
                                ),
                            ),
                        )
                    }
                    1u16 => {
                        Ok(
                            Which::GetPointerField(
                                <u16 as _p::field::FieldType>::accessor(
                                    &mut repr.0,
                                    &super::Op::GET_POINTER_FIELD.field,
                                ),
                            ),
                        )
                    }
                    unknown => Err(_p::NotInSchema(unknown)),
                }
            }
        }
    }
}
#[derive(Clone)]
pub struct ThirdPartyCapDescriptor<T = _p::Family>(T);
impl _p::ty::SchemaType for ThirdPartyCapDescriptor {
    const ID: u64 = 15235686326393111165u64;
}
impl<T> _p::IntoFamily for ThirdPartyCapDescriptor<T> {
    type Family = ThirdPartyCapDescriptor;
}
impl<T: _p::Capable> _p::Capable for ThirdPartyCapDescriptor<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = ThirdPartyCapDescriptor<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (ThirdPartyCapDescriptor(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr
for third_party_cap_descriptor::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for third_party_cap_descriptor::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        ThirdPartyCapDescriptor(ptr)
    }
}
impl<
    'a,
    T: _p::rpc::Table,
> core::convert::From<third_party_cap_descriptor::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: third_party_cap_descriptor::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for third_party_cap_descriptor::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader
for third_party_cap_descriptor::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr
for third_party_cap_descriptor::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<
    'a,
    T: _p::rpc::Table,
> core::convert::From<third_party_cap_descriptor::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: third_party_cap_descriptor::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for third_party_cap_descriptor::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for third_party_cap_descriptor::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder
for third_party_cap_descriptor::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for ThirdPartyCapDescriptor {
    type Reader<'a, T: _p::rpc::Table> = third_party_cap_descriptor::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = third_party_cap_descriptor::Builder<'a, T>;
}
impl _p::ty::Struct for ThirdPartyCapDescriptor {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 1u16,
    };
}
impl ThirdPartyCapDescriptor {
    const ID: _p::Descriptor<_p::AnyPtr> = _p::Descriptor::<_p::AnyPtr> {
        slot: 0u32,
        default: ::core::option::Option::None,
    };
    const VINE_ID: _p::Descriptor<u32> = _p::Descriptor::<u32> {
        slot: 0u32,
        default: 0u32,
    };
}
impl<'p, T: _p::rpc::Table + 'p> third_party_cap_descriptor::Reader<'p, T> {
    #[inline]
    pub fn id(&self) -> _p::Accessor<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(
                &self.0,
                &ThirdPartyCapDescriptor::ID,
            )
        }
    }
    #[inline]
    pub fn vine_id(&self) -> _p::Accessor<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(
                &self.0,
                &ThirdPartyCapDescriptor::VINE_ID,
            )
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> third_party_cap_descriptor::Builder<'p, T> {
    #[inline]
    pub fn id(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(
                &mut self.0,
                &ThirdPartyCapDescriptor::ID,
            )
        }
    }
    #[inline]
    pub fn vine_id(&mut self) -> _p::AccessorMut<'_, 'p, T, u32> {
        unsafe {
            <u32 as _p::field::FieldType>::accessor(
                &mut self.0,
                &ThirdPartyCapDescriptor::VINE_ID,
            )
        }
    }
    #[inline]
    pub fn into_id(self) -> _p::AccessorOwned<'p, T, _p::AnyPtr> {
        unsafe {
            <_p::AnyPtr as _p::field::FieldType>::accessor(
                self.0,
                &ThirdPartyCapDescriptor::ID,
            )
        }
    }
}
pub mod third_party_cap_descriptor {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::ThirdPartyCapDescriptor<
        _p::StructReader<'a, T>,
    >;
    pub type Builder<'a, T = _p::rpc::Empty> = super::ThirdPartyCapDescriptor<
        _p::StructBuilder<'a, T>,
    >;
}
#[derive(Clone)]
pub struct Exception<T = _p::Family>(T);
impl _p::ty::SchemaType for Exception {
    const ID: u64 = 15430940935639230746u64;
}
impl<T> _p::IntoFamily for Exception<T> {
    type Family = Exception;
}
impl<T: _p::Capable> _p::Capable for Exception<T> {
    type Table = T::Table;
    type Imbued = T::Imbued;
    type ImbuedWith<T2: _p::rpc::Table> = Exception<T::ImbuedWith<T2>>;
    #[inline]
    fn imbued(&self) -> &Self::Imbued {
        self.0.imbued()
    }
    #[inline]
    fn imbue_release<T2: _p::rpc::Table>(
        self,
        new_table: <Self::ImbuedWith<T2> as _p::Capable>::Imbued,
    ) -> (Self::ImbuedWith<T2>, Self::Imbued) {
        let (imbued, old) = self.0.imbue_release(new_table);
        (Exception(imbued), old)
    }
    #[inline]
    fn imbue_release_into<U>(&self, other: U) -> (U::ImbuedWith<Self::Table>, U::Imbued)
    where
        U: _p::Capable,
        U::ImbuedWith<Self::Table>: _p::Capable<Imbued = Self::Imbued>,
    {
        self.0.imbue_release_into(other)
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for exception::Reader<'a, T> {
    type Ptr = _p::StructReader<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<_p::StructReader<'a, T>>
for exception::Reader<'a, T> {
    #[inline]
    fn from(ptr: _p::StructReader<'a, T>) -> Self {
        Exception(ptr)
    }
}
impl<'a, T: _p::rpc::Table> core::convert::From<exception::Reader<'a, T>>
for _p::StructReader<'a, T> {
    #[inline]
    fn from(reader: exception::Reader<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructReader<'a, T>>
for exception::Reader<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructReader<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructReader for exception::Reader<'a, T> {}
impl<'a, T: _p::rpc::Table> _p::ty::TypedPtr for exception::Builder<'a, T> {
    type Ptr = _p::StructBuilder<'a, T>;
}
impl<'a, T: _p::rpc::Table> core::convert::From<exception::Builder<'a, T>>
for _p::StructBuilder<'a, T> {
    #[inline]
    fn from(reader: exception::Builder<'a, T>) -> Self {
        reader.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsRef<_p::StructBuilder<'a, T>>
for exception::Builder<'a, T> {
    #[inline]
    fn as_ref(&self) -> &_p::StructBuilder<'a, T> {
        &self.0
    }
}
impl<'a, T: _p::rpc::Table> core::convert::AsMut<_p::StructBuilder<'a, T>>
for exception::Builder<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut _p::StructBuilder<'a, T> {
        &mut self.0
    }
}
impl<'a, T: _p::rpc::Table> _p::ty::StructBuilder for exception::Builder<'a, T> {
    unsafe fn from_ptr(ptr: Self::Ptr) -> Self {
        Self(ptr)
    }
}
impl _p::StructView for Exception {
    type Reader<'a, T: _p::rpc::Table> = exception::Reader<'a, T>;
    type Builder<'a, T: _p::rpc::Table> = exception::Builder<'a, T>;
}
impl _p::ty::Struct for Exception {
    const SIZE: _p::StructSize = _p::StructSize {
        data: 1u16,
        ptrs: 2u16,
    };
}
impl Exception {
    const REASON: _p::Descriptor<_p::Text> = _p::Descriptor::<_p::Text> {
        slot: 0u32,
        default: ::core::option::Option::None,
    };
    const OBSOLETE_IS_CALLERS_FAULT: _p::Descriptor<bool> = _p::Descriptor::<bool> {
        slot: 0u32,
        default: false,
    };
    const OBSOLETE_DURABILITY: _p::Descriptor<u16> = _p::Descriptor::<u16> {
        slot: 1u32,
        default: 0u16,
    };
    const TYPE: _p::Descriptor<_p::Enum<exception::Type>> = _p::Descriptor::<
        _p::Enum<exception::Type>,
    > {
        slot: 2u32,
        default: exception::Type::Failed,
    };
    const TRACE: _p::Descriptor<_p::Text> = _p::Descriptor::<_p::Text> {
        slot: 1u32,
        default: ::core::option::Option::None,
    };
}
impl<'p, T: _p::rpc::Table + 'p> exception::Reader<'p, T> {
    #[inline]
    pub fn reason(&self) -> _p::Accessor<'_, 'p, T, _p::Text> {
        unsafe {
            <_p::Text as _p::field::FieldType>::accessor(&self.0, &Exception::REASON)
        }
    }
    #[inline]
    pub fn obsolete_is_callers_fault(&self) -> _p::Accessor<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &self.0,
                &Exception::OBSOLETE_IS_CALLERS_FAULT,
            )
        }
    }
    #[inline]
    pub fn obsolete_durability(&self) -> _p::Accessor<'_, 'p, T, u16> {
        unsafe {
            <u16 as _p::field::FieldType>::accessor(
                &self.0,
                &Exception::OBSOLETE_DURABILITY,
            )
        }
    }
    #[inline]
    pub fn r#type(&self) -> _p::Accessor<'_, 'p, T, _p::Enum<exception::Type>> {
        unsafe {
            <_p::Enum<
                exception::Type,
            > as _p::field::FieldType>::accessor(&self.0, &Exception::TYPE)
        }
    }
    #[inline]
    pub fn trace(&self) -> _p::Accessor<'_, 'p, T, _p::Text> {
        unsafe {
            <_p::Text as _p::field::FieldType>::accessor(&self.0, &Exception::TRACE)
        }
    }
}
impl<'p, T: _p::rpc::Table + 'p> exception::Builder<'p, T> {
    #[inline]
    pub fn reason(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::Text> {
        unsafe {
            <_p::Text as _p::field::FieldType>::accessor(&mut self.0, &Exception::REASON)
        }
    }
    #[inline]
    pub fn obsolete_is_callers_fault(&mut self) -> _p::AccessorMut<'_, 'p, T, bool> {
        unsafe {
            <bool as _p::field::FieldType>::accessor(
                &mut self.0,
                &Exception::OBSOLETE_IS_CALLERS_FAULT,
            )
        }
    }
    #[inline]
    pub fn obsolete_durability(&mut self) -> _p::AccessorMut<'_, 'p, T, u16> {
        unsafe {
            <u16 as _p::field::FieldType>::accessor(
                &mut self.0,
                &Exception::OBSOLETE_DURABILITY,
            )
        }
    }
    #[inline]
    pub fn r#type(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::Enum<exception::Type>> {
        unsafe {
            <_p::Enum<
                exception::Type,
            > as _p::field::FieldType>::accessor(&mut self.0, &Exception::TYPE)
        }
    }
    #[inline]
    pub fn trace(&mut self) -> _p::AccessorMut<'_, 'p, T, _p::Text> {
        unsafe {
            <_p::Text as _p::field::FieldType>::accessor(&mut self.0, &Exception::TRACE)
        }
    }
    #[inline]
    pub fn into_reason(self) -> _p::AccessorOwned<'p, T, _p::Text> {
        unsafe {
            <_p::Text as _p::field::FieldType>::accessor(self.0, &Exception::REASON)
        }
    }
    #[inline]
    pub fn into_trace(self) -> _p::AccessorOwned<'p, T, _p::Text> {
        unsafe {
            <_p::Text as _p::field::FieldType>::accessor(self.0, &Exception::TRACE)
        }
    }
}
pub mod exception {
    use super::{__file, __imports, _p};
    pub type Reader<'a, T = _p::rpc::Empty> = super::Exception<_p::StructReader<'a, T>>;
    pub type Builder<'a, T = _p::rpc::Empty> = super::Exception<
        _p::StructBuilder<'a, T>,
    >;
    #[repr(u16)]
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
    pub enum Type {
        #[default]
        Failed,
        Overloaded,
        Disconnected,
        Unimplemented,
    }
    impl _p::ty::SchemaType for Type {
        const ID: u64 = 12865824133959433560u64;
    }
    impl core::convert::TryFrom<u16> for Type {
        type Error = _p::NotInSchema;
        #[inline]
        fn try_from(value: u16) -> Result<Self, _p::NotInSchema> {
            match value {
                0u16 => Ok(Self::Failed),
                1u16 => Ok(Self::Overloaded),
                2u16 => Ok(Self::Disconnected),
                3u16 => Ok(Self::Unimplemented),
                value => Err(_p::NotInSchema(value)),
            }
        }
    }
    impl core::convert::From<Type> for u16 {
        #[inline]
        fn from(value: Type) -> Self {
            value as u16
        }
    }
    impl _p::ty::Enum for Type {}
}
