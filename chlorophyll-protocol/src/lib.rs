#![no_std]

// OK so we need a basic text protocol
// I'm thinking something along the line of
// ID COMMAND PAYLOAD
// so a sensor command could be
// 1 SENSOR 68 F

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
