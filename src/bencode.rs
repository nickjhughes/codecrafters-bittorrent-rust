use anyhow::Result;
use std::collections::BTreeMap;

#[derive(Debug, PartialEq)]
pub enum BencodeValue<'input> {
    ByteString(BencodeByteString<'input>),
    Integer(i64),
    List(Vec<BencodeValue<'input>>),
    Dictionary(BTreeMap<BencodeByteString<'input>, BencodeValue<'input>>),
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct BencodeByteString<'input>(pub &'input [u8]);

impl std::fmt::Display for BencodeByteString<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match std::str::from_utf8(self.0) {
            Ok(s) => {
                write!(f, "{:?}", s)
            }
            Err(_) => {
                write!(f, "{:?}", self.0)
            }
        }
    }
}

impl std::fmt::Display for BencodeValue<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BencodeValue::ByteString(bs) => write!(f, "{}", bs),
            BencodeValue::Integer(n) => write!(f, "{}", n),
            BencodeValue::List(values) => {
                write!(f, "[")?;
                for (i, value) in values.iter().enumerate() {
                    write!(f, "{}", value)?;
                    if i < values.len() - 1 {
                        write!(f, ",")?;
                    }
                }
                write!(f, "]")
            }
            BencodeValue::Dictionary(map) => {
                write!(f, "{{")?;
                for (i, (key, value)) in map.iter().enumerate() {
                    write!(f, "{}:{}", key, value)?;
                    if i < map.len() - 1 {
                        write!(f, ",")?;
                    }
                }
                write!(f, "}}")
            }
        }
    }
}

impl<'input> BencodeValue<'input> {
    pub fn from_str(input: &'input str) -> Result<(&str, Self)> {
        let (rest, value) = BencodeValue::from_bytes(input.as_bytes())?;
        Ok((std::str::from_utf8(rest)?, value))
    }

    pub fn from_bytes(input: &'input [u8]) -> Result<(&[u8], Self)> {
        match input[0] {
            b'0'..=b'9' => {
                // Byte string
                let delimiter_index = input.iter().position(|b| *b == b':');
                match delimiter_index {
                    Some(delimiter_index) => {
                        let length =
                            std::str::from_utf8(&input[0..delimiter_index])?.parse::<usize>()?;
                        if delimiter_index + 1 + length > input.len() {
                            anyhow::bail!("premature end of byte string");
                        }
                        let value = &input[delimiter_index + 1..delimiter_index + 1 + length];
                        Ok((
                            &input[delimiter_index + 1 + length..],
                            BencodeValue::ByteString(BencodeByteString(value)),
                        ))
                    }
                    None => anyhow::bail!("premature end of byte string"),
                }
            }
            b'i' => {
                // Integer
                let end_index = input.iter().position(|b| *b == b'e');
                match end_index {
                    Some(end_index) => {
                        // TODO: Leading zeros and negative zero are not allowed, but we accept them here
                        let value = std::str::from_utf8(&input[1..end_index])?.parse::<i64>()?;
                        Ok((&input[end_index + 1..], BencodeValue::Integer(value)))
                    }
                    None => anyhow::bail!("premature end of integer"),
                }
            }
            b'l' => {
                // List
                let mut values = Vec::new();
                let mut rest = &input[1..];
                loop {
                    match rest.first() {
                        None => anyhow::bail!("premature end of list"),
                        Some(b'e') => break,
                        _ => {
                            let (remainder, value) = BencodeValue::from_bytes(rest)?;
                            rest = remainder;
                            values.push(value);
                        }
                    }
                }
                Ok((&rest[1..], BencodeValue::List(values)))
            }
            b'd' => {
                // Dictionary
                let mut map = BTreeMap::new();
                let mut rest = &input[1..];
                loop {
                    match rest.first() {
                        None => anyhow::bail!("premature end of dictionary"),
                        Some(b'e') => break,
                        _ => {
                            let (remainder, key) = BencodeValue::from_bytes(rest)?;
                            rest = remainder;
                            let (remainder, value) = BencodeValue::from_bytes(rest)?;
                            rest = remainder;
                            match key {
                                BencodeValue::ByteString(byte_string) => {
                                    map.insert(byte_string, value);
                                }
                                _ => anyhow::bail!("non-byte string dictionary key"),
                            }
                        }
                    }
                }
                Ok((&rest[1..], BencodeValue::Dictionary(map)))
            }
            _ => anyhow::bail!("invalid bencode value"),
        }
    }

    pub fn as_byte_string(&self) -> Option<&BencodeByteString> {
        match self {
            BencodeValue::ByteString(bs) => Some(bs),
            _ => None,
        }
    }

    pub fn as_dictionary(&self) -> Option<&BTreeMap<BencodeByteString, BencodeValue>> {
        match self {
            BencodeValue::Dictionary(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<&i64> {
        match self {
            BencodeValue::Integer(n) => Some(n),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_list(&self) -> Option<&[BencodeValue]> {
        match self {
            BencodeValue::List(values) => Some(values),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BencodeByteString, BencodeValue};
    use std::collections::BTreeMap;

    #[test]
    fn parse_integer() {
        {
            // Positive integer
            let input = "i42e";
            let (rest, value) = BencodeValue::from_str(input).unwrap();
            assert!(rest.is_empty());
            assert_eq!(value, BencodeValue::Integer(42));
        }

        {
            // Zero
            let input = "i0e";
            let (rest, value) = BencodeValue::from_str(input).unwrap();
            assert!(rest.is_empty());
            assert_eq!(value, BencodeValue::Integer(0));
        }

        {
            // Negative integer
            let input = "i-123e";
            let (rest, value) = BencodeValue::from_str(input).unwrap();
            assert!(rest.is_empty());
            assert_eq!(value, BencodeValue::Integer(-123));
        }

        {
            // Missing end delimiter
            let input = "i42";
            let result = BencodeValue::from_str(input);
            assert!(result.is_err());
        }
    }

    #[test]
    fn parse_byte_string() {
        {
            // Normal string
            let input = "5:hello";
            let (rest, value) = BencodeValue::from_str(input).unwrap();
            assert!(rest.is_empty());
            assert_eq!(value, BencodeValue::ByteString(BencodeByteString(b"hello")));
        }

        {
            // Non utf-8 string
            let input = b"1:\xEF";
            let (rest, value) = BencodeValue::from_bytes(input).unwrap();
            assert!(rest.is_empty());
            assert_eq!(value, BencodeValue::ByteString(BencodeByteString(b"\xEF")));
        }

        {
            let input = "5:foo";
            let result = BencodeValue::from_str(input);
            assert!(result.is_err());
        }

        {
            let input = "5";
            let result = BencodeValue::from_str(input);
            assert!(result.is_err());
        }
    }

    #[test]
    fn parse_list() {
        {
            // Normal list
            let input = "l4:spami42ee";
            let (rest, value) = BencodeValue::from_str(input).unwrap();
            assert!(rest.is_empty());
            assert_eq!(
                value,
                BencodeValue::List(vec![
                    BencodeValue::ByteString(BencodeByteString(b"spam")),
                    BencodeValue::Integer(42),
                ])
            );
        }

        {
            // Nested list
            let input = "l4:spaml3:fooi0eei42ee";
            let (rest, value) = BencodeValue::from_str(input).unwrap();
            assert!(rest.is_empty());
            assert_eq!(
                value,
                BencodeValue::List(vec![
                    BencodeValue::ByteString(BencodeByteString(b"spam")),
                    BencodeValue::List(vec![
                        BencodeValue::ByteString(BencodeByteString(b"foo")),
                        BencodeValue::Integer(0),
                    ]),
                    BencodeValue::Integer(42),
                ])
            );
        }

        {
            // Missing end delimiter
            let input = "l4:spami42e";
            let result = BencodeValue::from_str(input);
            assert!(result.is_err());
        }
    }

    #[test]
    fn parse_dictionary() {
        {
            // Normal dictionary
            let input = "d3:bar4:spam3:fooi42ee";
            let (rest, value) = BencodeValue::from_str(input).unwrap();
            assert!(rest.is_empty());
            assert_eq!(
                value,
                BencodeValue::Dictionary(
                    [
                        (
                            BencodeByteString(b"bar"),
                            BencodeValue::ByteString(BencodeByteString(b"spam"))
                        ),
                        (BencodeByteString(b"foo"), BencodeValue::Integer(42))
                    ]
                    .into_iter()
                    .collect::<BTreeMap<_, _>>()
                )
            );
        }

        {
            // Non-byte string key
            let input = "di42e3:fooe";
            let result = BencodeValue::from_str(input);
            assert!(result.is_err());
        }

        {
            // Missing end delimiter
            let input = "d4:spami42";
            let result = BencodeValue::from_str(input);
            assert!(result.is_err());
        }
    }
}
