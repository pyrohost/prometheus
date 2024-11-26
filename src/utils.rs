#[macro_export]
macro_rules! default_struct {
    (
        $(#[$struct_meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $type:ty $(= $default:expr)?
            ),* $(,)?
        }
    ) => {
        $(#[$struct_meta])*
        $vis struct $name {
            $(
                $(#[$field_meta])*
                $field_vis $field: $type
            ),*
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    $(
                        $field: $crate::default_struct!(@default $($default)?)
                    ),*
                }
            }
        }
    };
    (@default) => {
        Default::default()
    };
    (@default $expr:expr) => {
        $expr
    };
}
