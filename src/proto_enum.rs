/// Declares a protobuf enum that serialises by its wire name, matching protobufjs `toObject({enums: String})`.
macro_rules! proto_enum {
    ($(#[$meta:meta])* pub enum $name:ident { $($variant:ident = $code:literal => $wire:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[allow(dead_code, reason = "wire enums mirror the whole protocol, not just what this app sends")]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name { $($variant = $code),+ }

        #[allow(dead_code, reason = "not every wire enum needs its numeric code")]
        impl $name {
            pub fn code(self) -> i32 {
                self as i32
            }

            pub fn wire_name(self) -> &'static str {
                match self { $($name::$variant => $wire),+ }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.wire_name())
            }
        }
    };
}
