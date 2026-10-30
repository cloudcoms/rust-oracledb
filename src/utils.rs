//-----------------------------------------------------------------------------
// Copyright (c) 2026, Oracle and/or its affiliates.
//
// This software is dual-licensed to you under the Universal Permissive License
// (UPL) 1.0 as shown at https://oss.oracle.com/licenses/upl and Apache License
// 2.0 as shown at http://www.apache.org/licenses/LICENSE-2.0. You may choose
// either license.
//
// If you elect to accept the software under the Apache License, Version 2.0,
// the following applies:
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------
// utils.rs
//
// Defines a number of utility functions.
//-----------------------------------------------------------------------------

use crate::constants;
use crate::error::Error;
use crate::parser::Parser;

/// Encodes a Rust string as big-endian UTF-16 bytes.
pub(crate) fn string_to_utf16be_bytes(value: &str) -> Vec<u8> {
    let mut bytes = Vec::<u8>::new();
    for code_unit in value.encode_utf16() {
        bytes.extend_from_slice(&code_unit.to_be_bytes());
    }
    bytes
}

/// Encodes a Rust string as little-endian UTF-16 bytes.
pub(crate) fn string_to_utf16le_bytes(value: &str) -> Vec<u8> {
    let mut bytes = Vec::<u8>::new();
    for code_unit in value.encode_utf16() {
        bytes.extend_from_slice(&code_unit.to_le_bytes());
    }
    bytes
}

/// Decodes big-endian UTF-16 bytes into a Rust string.
pub(crate) fn utf16be_bytes_to_string(bytes: &[u8]) -> Result<String, Error> {
    let num_code_units = bytes.len() / 2;
    let mut code_units = Vec::<u16>::with_capacity(num_code_units);
    for i in 0..num_code_units {
        let offset = i * 2;
        let code_unit =
            u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap());
        code_units.push(code_unit);
    }
    Ok(String::from_utf16(&code_units)?)
}

/// Decodes CESU-8 bytes (Oracle's "UTF8" character set) into a Rust string.
/// CESU-8 matches UTF-8 except that a supplementary character is sent as two
/// 3-byte encoded UTF-16 surrogates instead of one 4-byte sequence.
pub(crate) fn cesu8_bytes_to_string(bytes: &[u8]) -> Result<String, Error> {
    let mut value = String::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        let error = match std::str::from_utf8(rest) {
            Ok(valid) => {
                value.push_str(valid);
                return Ok(value);
            }
            Err(error) => error,
        };
        let (valid, tail) = rest.split_at(error.valid_up_to());
        value.push_str(std::str::from_utf8(valid)?);
        match tail {
            [
                0xed,
                high @ 0xa0..=0xaf,
                high_low @ 0x80..=0xbf,
                0xed,
                low @ 0xb0..=0xbf,
                low_low @ 0x80..=0xbf,
                ..,
            ] => {
                let high =
                    (u32::from(high & 0x0f) << 6) | u32::from(high_low & 0x3f);
                let low =
                    (u32::from(low & 0x0f) << 6) | u32::from(low_low & 0x3f);
                let code_point = 0x10000 + (high << 10) + low;
                value.push(char::from_u32(code_point).unwrap());
                rest = &tail[6..];
            }
            _ => return Err(error.into()),
        }
    }
}

/// Decodes NCHAR data, which the database sends in its national character
/// set rather than converting it like CHAR data.
pub(crate) fn nchar_bytes_to_string(
    bytes: &[u8],
    ncharset_id: u16,
) -> Result<String, Error> {
    if ncharset_id == constants::CHARSET_ID_CESU8 {
        cesu8_bytes_to_string(bytes)
    } else {
        utf16be_bytes_to_string(bytes)
    }
}

/// Decodes little-endian UTF-16 bytes into a Rust string.
pub(crate) fn utf16le_bytes_to_string(bytes: &[u8]) -> Result<String, Error> {
    let num_code_units = bytes.len() / 2;
    let mut code_units = Vec::<u16>::with_capacity(num_code_units);
    for i in 0..num_code_units {
        let offset = i * 2;
        let code_unit =
            u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
        code_units.push(code_unit);
    }
    Ok(String::from_utf16(&code_units)?)
}

/// Returns the supplied string as a new string that is safe to include as a
/// literal in a SQL statement.
pub fn enquote_literal(value: &str) -> String {
    let mut result = String::new();
    result.push('\'');
    for ch in value.chars() {
        result.push(ch);
        if ch == '\'' {
            result.push(ch);
        }
    }
    result.push('\'');
    result
}

/// Returns the supplied string as a new string that is safe to include as a
/// quoted name in a SQL statement. Since embedded quotes are not allowed in
/// quoted identifiers, an error is returned if embedded quotes are detected.
/// If the capitalize parameter is true, the string is converted to uppercase.
pub fn enquote_name(value: &str, capitalize: bool) -> Result<String, Error> {
    if value.contains('"') {
        Err(Error::name_has_embedded_quotes())
    } else {
        let adjusted_value = if capitalize {
            value.to_uppercase()
        } else {
            value.to_string()
        };
        Ok(format!("\"{}\"", adjusted_value))
    }
}

/// Returns a boolean indicating if the supplied string contains a valid
/// qualified SQL name. Leading and trailing spaces are ignored. The name must
/// be one or more simple SQL names separated by periods (and any amount of
/// whitespace), optionally followed by the '@' symbol and an additional simple
/// SQL name referring to a database link name.
pub fn is_qualified_sql_name(value: &str) -> bool {
    let mut num_parts = 0;
    let mut parser = Parser::new(value.trim());
    while parser.parse_simple_sql_name().is_some() {
        num_parts += 1;
        parser.skip_whitespace();
        let ch_opt = parser.next_char();
        parser.skip_whitespace();
        match ch_opt {
            Some(ch) => {
                if ch == '@' {
                    return parser.parse_simple_sql_name().is_some()
                        && parser.next_char().is_none();
                } else if ch != '.' {
                    return false;
                }
            }
            None => break,
        }
    }
    num_parts > 0 && parser.next_char().is_none()
}

/// Returns a boolean indicating if the supplied string contains a valid simple
/// SQL name. Leading and trailing spaces are ignored. If the value is not
/// quoted, the first character must be alphabetic and the remaining characters
/// must be alphanumeric or contain the characters '_', '$' or '#'. A quoted
/// name may not contain embedded quotes and no characters other than
/// whitespace are allowed outside the quotes.
pub fn is_simple_sql_name(value: &str) -> bool {
    let mut parser = Parser::new(value.trim());
    parser.parse_simple_sql_name().is_some() && parser.next_char().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cesu8_decodes_bmp_text_like_utf8() {
        let text = "id 124936 센서 설명";
        assert_eq!(cesu8_bytes_to_string(text.as_bytes()).unwrap(), text);
        assert_eq!(cesu8_bytes_to_string(b"").unwrap(), "");
    }

    #[test]
    fn cesu8_decodes_surrogate_pairs_as_one_character() {
        // U+1F600 is the surrogate pair D83D DE00 in CESU-8
        let bytes = b"a\xed\xa0\xbd\xed\xb8\x80\xea\xb0\x80";
        assert_eq!(cesu8_bytes_to_string(bytes).unwrap(), "a\u{1f600}가");
        // the first and last supplementary characters
        let edges = b"\xed\xa0\x80\xed\xb0\x80\xed\xaf\xbf\xed\xbf\xbf";
        assert_eq!(
            cesu8_bytes_to_string(edges).unwrap(),
            "\u{10000}\u{10ffff}"
        );
    }

    #[test]
    fn cesu8_accepts_four_byte_utf8() {
        let text = "x\u{1f600}y";
        assert_eq!(cesu8_bytes_to_string(text.as_bytes()).unwrap(), text);
    }

    #[test]
    fn cesu8_rejects_unpaired_surrogates_and_bad_bytes() {
        for bytes in [
            &b"\xed\xa0\xbd"[..],
            b"\xed\xa0\xbdz",
            b"\xed\xb8\x80\xed\xa0\xbd",
            b"\xff",
            b"\xea\xb0",
        ] {
            assert!(cesu8_bytes_to_string(bytes).is_err(), "{bytes:x?}");
        }
    }

    #[test]
    fn nchar_decoding_follows_the_national_character_set() {
        let utf16 = string_to_utf16be_bytes("한글");
        assert_eq!(nchar_bytes_to_string(&utf16, 2000).unwrap(), "한글");
        let cesu8 = "한글".as_bytes();
        assert_eq!(
            nchar_bytes_to_string(cesu8, constants::CHARSET_ID_CESU8).unwrap(),
            "한글"
        );
    }
}
