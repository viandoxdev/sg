#![allow(dead_code)]
#![feature(proc_macro_diagnostic)]
#![feature(proc_macro_span_shrink)]

use proc_macro2::{Ident, Span};
use quote::quote;
use syn::{parse::Parse, parse_macro_input, Index, LitInt};

struct Count {
    count: u64,
}

impl Parse for Count {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let lookahead = input.lookahead1();
        if lookahead.peek(LitInt) {
            let lit: LitInt = input.parse()?;
            Ok(Self {
                count: lit.base10_parse::<u64>()?,
            })
        } else {
            Err(lookahead.error())
        }
    }
}

fn n_to_type(mut n: u64, total: u64) -> Ident {
    const LETTERS: [char; 26] = [
        'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R',
        'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
    ];
    const NUMBERS: [char; 10] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];
    let len = if total < 26 {
        1
    } else {
        2 + (total as f32 / 26.0).log10() as u64
    };
    let lt = LETTERS[n as usize % 26];
    let mut res = String::new();
    n /= 26;
    for _ in 1..len {
        let r = n % 10;
        res.push(NUMBERS[r as usize]);
        n /= 10;
    }
    res = res.chars().rev().collect();
    res.insert(0, lt);
    Ident::new(&res, Span::call_site())
}

#[proc_macro]
pub fn impl_archetype(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let count = parse_macro_input!(input as Count).count;
    let output = {
        let impls = (0..=count).map(|count| {
            // eg "2"
            let cap = count as usize;
            // eg "A", "B"
            let types = (0..count).map(|v| n_to_type(v, count));
            // eg "0", "1"
            let indices = (0..count).map(|v| Index::from(v as usize));
            // eg "(A, B,)"
            let tuple = if count > 0{
                let types = types.clone();
                quote!((#(#types),*,))
            } else {
                quote!(())
            };
            // eg "A: 'static, B: 'static"
            let generics = if count > 0 {
                let types = types.clone();
                quote!(<#(#types: 'static),*>)
            } else {
                quote!()
            };
            let matches = {
                let types = types.clone();
                quote!(#(&& archetype.has::<#types>())*)
            };
            let writes = {
                let types = types.clone();
                let indices = indices.clone();
                quote!{#(
                    std::ptr::write(dst.add(archetype.offset::<#types>()) as *mut #types, self.#indices);
                )*}
            };
            let reads = {
                let types = types.clone();
                let indices = indices.clone();
                quote!{#(
                    std::ptr::copy(src.add(archetype.offset::<#types>()) as *const #types, &mut value.#indices as *mut #types, 1);
                )*}
            };
            let typeids = {
                let types = types.clone();
                quote!{#(
                    TypeId::of::<#types>()
                ),*}
            };
            let adds = {
                let types = types.clone();
                quote!(#(.add::<#types>())*)
            };
            quote!{
                impl #generics IntoArchetype for #tuple {
                    fn into_archetype() -> Archetype {
                        let layout = Layout::new::<#tuple>();
                        let mut info = HashMap::with_capacity(#cap);

                        unsafe {
                            let ptr = std::ptr::null::<#tuple>();
                            #(
                                info.insert(TypeId::of::<#types>(), ComponentType {
                                    // ptr is a null pointer, so the offset from any pointer p from
                                    // it is the pointer p's value (-> as usize gives offset)
                                    offset: (&(*ptr).#indices as *const #types) as usize,
                                    drop: match std::mem::needs_drop::<#types>() {
                                        true => Some(get_drop::<#types>()),
                                        false => None,
                                    },
                                    size: std::mem::size_of::<#types>(),
                                    alignment: std::mem::align_of::<#types>(),
                                });
                            )*
                        }

                        Archetype {
                            info,
                            layout,
                        }
                    }
                    fn match_archetype(archetype: &Archetype) -> bool {
                        if archetype.info.len() == #cap {
                            Self::archetype_contains(archetype)
                        } else {
                            false
                        }
                    }
                    fn archetype_contains(archetype: &Archetype) -> bool {
                        true #matches
                    }
                    fn bitset(builder: &mut BitsetBuilder) -> Option<ArchetypeBitset> {
                        builder.start_archetype()
                            #adds
                            .build_archetype()
                    }
                    unsafe fn write(self, dst: *mut u8, archetype: &Archetype) {
                        #[cfg(debug_assertions)]
                        if !Self::archetype_contains(archetype) {
                            panic!("Archetypes do not match");
                        }
                        #writes
                        // We dont forget self, because it is already moved by the writes (partial
                        // moves of every field -> complete move)
                    }
                    unsafe fn read(src: *const u8, archetype: &Archetype) -> Self {
                        #[cfg(debug_assertions)]
                        if !Self::archetype_contains(archetype) {
                            panic!("Archetypes do not match");
                        }
                        let mut value: Self = MaybeUninit::uninit().assume_init();
                        #reads
                        value
                    }
                    fn types() -> Vec<TypeId> {
                        vec![
                            #typeids
                        ]
                    }
                }
            }
        });
        quote! {
            #(#impls)*
        }
    };
    output.into()
}

#[proc_macro]
pub fn impl_query(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let count = parse_macro_input!(input as Count).count;
    let output = {
        let impls = (1..=count).map(|count| {
            // eg "A", "B"
            let types = (0..count).map(|v| n_to_type(v, count));
            // eg "(A, B,)"
            let tuple = {
                let types = types.clone();
                quote!((#(#types),*,))
            };
            // eg "A: 'static, B: 'static"
            let generics = {
                let types = types.clone();
                quote!(<#(#types: Query),*>)
            };
            let matches = {
                let types = types.clone();
                quote!(#(&& #types::match_archetype(archetype))*)
            };
            let builds = {
                let types = types.clone();
                quote!(#(#types::build_bitset(builder);)*)
            };
            quote! {
                impl #generics Query for #tuple {
                    fn match_archetype(archetype: &Archetype) -> bool {
                        true #matches
                    }
                    fn build(ptr: *mut u8, archetype: &Archetype) -> Self {
                        (
                            #(#types::build(ptr, archetype)),*,
                        )
                    }
                    fn build_bitset(builder: &mut BitsetBuilder) {
                        #builds
                    }
                }
            }
        });
        quote! {
            #(#impls)*
        }
    };
    output.into()
}
