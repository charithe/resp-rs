use std::error::Error as StdError;
use std::fmt;
use std::io;

#[derive(Debug)]
pub enum Error {
    BadInteger(std::num::ParseIntError),
    BadString(std::string::FromUtf8Error),
    EndOfStream,
    IoError(io::Error),
    UnexpectedToken(char),
    UnknownError,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::BadInteger(err) => f.write_fmt(format_args!("Bad integer: {}", err)),
            Error::BadString(err) => f.write_fmt(format_args!("Bad string: {}", err)),
            Error::EndOfStream => f.write_str("End of stream"),
            Error::UnexpectedToken(tok) => f.write_fmt(format_args!("Unexpected token: {}", tok)),
            Error::IoError(err) => f.write_fmt(format_args!("IO error: {}", err)),
            Error::UnknownError => f.write_str("Unknown error"),
        }
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        "description"
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::IoError(err)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(err: std::string::FromUtf8Error) -> Error {
        Error::BadString(err)
    }
}

impl From<std::num::ParseIntError> for Error {
    fn from(err: std::num::ParseIntError) -> Error {
        Error::BadInteger(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, PartialEq)]
pub enum RESPType {
    SimpleString(String),
    Error(String),
    Integer(i64),
    BulkString(Vec<u8>),
    Null,
    Array(Vec<RESPType>),
}

pub struct Parser<R: io::Read> {
    bytes: io::Bytes<R>,
}

impl<R: io::Read> Parser<R> {
    pub fn new(bytes: io::Bytes<R>) -> Parser<R> {
        Parser { bytes }
    }

    pub fn parse_next(&mut self) -> Result<RESPType> {
        let byte = self.bytes.next().transpose()?;
        byte.map(|b| match b as char {
            '*' => self.parse_array(),
            '$' => self.parse_bulk_str(),
            '-' => self.parse_error(),
            ':' => self.parse_integer(),
            '+' => self.parse_simple_str(),
            v => Err(Error::UnexpectedToken(v)),
        })
        .unwrap_or(Err(Error::EndOfStream))
    }

    fn parse_array(&mut self) -> Result<RESPType> {
        let len = self.parse_integer()?;
        match len {
            RESPType::Integer(-1) => Ok(RESPType::Null),
            RESPType::Integer(n) => {
                let mut array: Vec<RESPType> = Vec::new();
                for _ in 0..n {
                    let item = self.parse_next()?;
                    array.push(item);
                }
                Ok(RESPType::Array(array))
            }
            _ => Err(Error::UnknownError),
        }
    }

    fn parse_bulk_str(&mut self) -> Result<RESPType> {
        let len = self.parse_integer()?;
        match len {
            RESPType::Integer(-1) => Ok(RESPType::Null),
            RESPType::Integer(n) if n >= 0 => {
                let mut buf: Vec<u8> = Vec::new();
                for _ in 0..n {
                    let byte = self.bytes.next().transpose()?;
                    match byte {
                        Some(b) => buf.push(b),
                        None => return Err(Error::EndOfStream),
                    }
                }
                self.read_to_crlf()?;
                Ok(RESPType::BulkString(buf))
            }
            _ => Err(Error::UnknownError),
        }
    }

    fn parse_error(&mut self) -> Result<RESPType> {
        let s = self.parse_simple_str()?;
        match s {
            RESPType::SimpleString(x) => Ok(RESPType::Error(x)),
            _ => Err(Error::UnknownError),
        }
    }

    fn parse_integer(&mut self) -> Result<RESPType> {
        let s = self.parse_simple_str()?;
        match s {
            RESPType::SimpleString(x) => {
                let i = x.parse::<i64>()?;
                Ok(RESPType::Integer(i))
            }
            _ => Err(Error::UnknownError),
        }
    }

    fn parse_simple_str(&mut self) -> Result<RESPType> {
        let buf = self.read_to_crlf()?;
        let s = String::from_utf8(buf)?;
        Ok(RESPType::SimpleString(s))
    }

    fn read_to_crlf(&mut self) -> Result<Vec<u8>> {
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let b = self.bytes.next().transpose()?;
            match b {
                Some(10) => break,
                Some(13) => {}
                Some(v) => buf.push(v),
                None => return Err(Error::EndOfStream),
            }
        }
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn do_parse(expr: &str) -> Result<RESPType> {
        let mut parser = Parser::new(expr.as_bytes().bytes());
        parser.parse_next()
    }

    macro_rules! test_parse_ok {
        ($name:ident, $input:literal, $want:expr) => {
            #[test]
            fn $name() {
                let have = do_parse($input);
                match have {
                    Ok(ref x) if *x == $want => assert!(true),
                    _ => assert!(false),
                }
            }
        };
    }

    macro_rules! test_parse_fail {
        ($name:ident, $input:literal) => {
            #[test]
            fn $name() {
                let have = do_parse($input);
                match have {
                    Err(_) => assert!(true),
                    _ => assert!(false),
                }
            }
        };
    }

    test_parse_ok!(valid_integer, ":32\r\n", RESPType::Integer(32));

    test_parse_ok!(
        valid_simple_string,
        "+TEST\r\n",
        RESPType::SimpleString("TEST".to_string())
    );

    test_parse_ok!(
        valid_error,
        "-ERROR\r\n",
        RESPType::Error("ERROR".to_string())
    );

    test_parse_ok!(
        valid_bulk_string,
        "$5\r\nHE\rHE\r\n",
        RESPType::BulkString(vec!('H' as u8, 'E' as u8, '\r' as u8, 'H' as u8, 'E' as u8))
    );

    test_parse_ok!(valid_null_bulk_string, "$-1\r\n", RESPType::Null);

    test_parse_ok!(
        valid_empty_bulk_string,
        "$0\r\n\r\n",
        RESPType::BulkString(Vec::new())
    );

    test_parse_ok!(
        valid_array,
        "*3\r\n:42\r\n+TEST\r\n$3\r\nXYZ\r\n\r\n",
        RESPType::Array(vec!(
            RESPType::Integer(42),
            RESPType::SimpleString("TEST".to_string()),
            RESPType::BulkString(vec!('X' as u8, 'Y' as u8, 'Z' as u8))
        ))
    );

    test_parse_ok!(
        valid_nested_array,
        "*1\r\n*3\r\n:42\r\n+TEST\r\n$3\r\nXYZ\r\n\r\n\r\n",
        RESPType::Array(vec!(RESPType::Array(vec!(
            RESPType::Integer(42),
            RESPType::SimpleString("TEST".to_string()),
            RESPType::BulkString(vec!('X' as u8, 'Y' as u8, 'Z' as u8))
        ))))
    );

    test_parse_ok!(valid_null_array, "*-1\r\n", RESPType::Null);

    test_parse_ok!(valid_empty_array, "*0\r\n", RESPType::Array(Vec::new()));

    test_parse_ok!(
        parses_only_one_item,
        ":32\r\n:42\r\n",
        RESPType::Integer(32)
    );

    test_parse_fail!(empty_input, "");

    test_parse_fail!(invalid_integer, ":ten\r\n");

    test_parse_fail!(no_delimiter, ":10");

    test_parse_fail!(bad_array, "*2\r\n+x\r\n\r\n");
}
