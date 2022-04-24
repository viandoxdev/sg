pub trait IntoString {
    fn into_string(self) -> String;
}

impl<T: ToString> IntoString for T {
    default fn into_string(self) -> String {
        self.to_string()
    }
}

impl IntoString for String {
    fn into_string(self) -> String {
        self
    }
}
