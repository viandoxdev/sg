#[macro_export]
macro_rules! numeric_enum {
    // Create the impl blocks for an enum
    (@impl $name:ident: $ty:ty { $($varient:ident = $value:literal),*$(,)? }) => {
        impl TryFrom<$ty> for $name {
            type Error = anyhow::Error;
            fn try_from(value: $ty) -> anyhow::Result<Self> {
                match value {
                    $($value => {Ok($name::$varient)}),*
                    c => Err(anyhow::anyhow!("Unknown {}: {c}", stringify!($name)))
                }
            }
        }

        impl From<$name> for $ty {
            fn from(value: $name) -> Self {
                value as Self
            }
        }

        impl $crate::chess::serialization::Serialize for $name {
            fn serialize(&self, bytes: &mut Vec<u8>) -> anyhow::Result<()> {
                <$ty>::from(*self).serialize(bytes)
            }
        }

        impl $crate::chess::serialization::Deserialize for $name {
            fn deserialize(bytes: &mut std::io::Cursor<Vec<u8>>) -> anyhow::Result<Self> {
                <$name>::try_from(<$ty>::deserialize(bytes)?)
            }
        }
    };
    // define and impl an enum
    (enum $name:ident: $ty:ty { $($varient:ident = $value:literal),*$(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum $name {
            $($varient = $value),*
        }

        numeric_enum! {
            @impl $name: $ty {
                $($varient = $value),*
            }
        }
    };
    // define and impl an enum with a visibility
    ($vis:vis enum $name:ident: $ty:ty { $($varient:ident = $value:literal),*$(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        $vis enum $name {
            $($varient = $value),*
        }

        numeric_enum! {
            @impl $name: $ty {
                $($varient = $value),*
            }
        }
    };
    // define and impl many enums with or without visibility
    ($($($t:tt)? enum $name:ident: $ty:ty { $($varient:ident = $value:literal),*$(,)? })*) => {
        $(
            numeric_enum! {
                $($t)? enum $name: $ty {
                    $($varient = $value),*
                }
            }
        )*
    };
}
