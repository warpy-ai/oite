pub const ABI_VERSION: u32 = 1;

pub const ABI_NAME: &str = "tscl";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_version() {
        assert_eq!(ABI_VERSION, 1);
    }

    #[test]
    fn test_abi_name() {
        assert_eq!(ABI_NAME, "tscl");
    }
}
