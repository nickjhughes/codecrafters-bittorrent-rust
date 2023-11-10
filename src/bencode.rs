use anyhow::Result;
use std::collections::HashMap;

// TODO: Borrow all these values, rather than reallocating
#[derive(Debug, PartialEq, Eq)]
pub enum BencodeValue {
    ByteString(BencodeByteString),
    Integer(i64),
    List(Vec<BencodeValue>),
    Dictionary(HashMap<BencodeByteString, BencodeValue>),
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub struct BencodeByteString(Vec<u8>);

impl BencodeValue {
    pub fn from_str(input: &str) -> Result<(&str, Self)> {
        let (rest, value) = BencodeValue::from_bytes(input.as_bytes())?;
        Ok((std::str::from_utf8(rest)?, value))
    }

    pub fn from_bytes(input: &[u8]) -> Result<(&[u8], Self)> {
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
                        let value =
                            input[delimiter_index + 1..delimiter_index + 1 + length].to_vec();
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
                let mut map = HashMap::new();
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
}

#[cfg(test)]
mod tests {
    use super::{BencodeByteString, BencodeValue};
    use std::collections::HashMap;

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
            assert_eq!(
                value,
                BencodeValue::ByteString(BencodeByteString(b"hello".to_vec()))
            );
        }

        {
            // Non utf-8 string
            let input = b"1:\xEF";
            let (rest, value) = BencodeValue::from_bytes(input).unwrap();
            assert!(rest.is_empty());
            assert_eq!(
                value,
                BencodeValue::ByteString(BencodeByteString(b"\xEF".to_vec()))
            );
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
                    BencodeValue::ByteString(BencodeByteString(b"spam".to_vec())),
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
                    BencodeValue::ByteString(BencodeByteString(b"spam".to_vec())),
                    BencodeValue::List(vec![
                        BencodeValue::ByteString(BencodeByteString(b"foo".to_vec())),
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
                            BencodeByteString(b"bar".to_vec()),
                            BencodeValue::ByteString(BencodeByteString(b"spam".to_vec()))
                        ),
                        (
                            BencodeByteString(b"foo".to_vec()),
                            BencodeValue::Integer(42)
                        )
                    ]
                    .into_iter()
                    .collect::<HashMap<_, _>>()
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
