/*!re2c
  re2c:api = record;
  re2c:YYCTYPE = u8;
  re2c:eof = 255;
  re2c:YYFILL = "fill(&mut yyrecord) == Fill::Ok";
*/

use crate::text::Reader;
use crate::scanner::{State, Fill, fill};


#[allow(warnings)] 
pub fn scan_email_reader<'a, T: Reader<'a>>(reader: &'a mut T) -> Option<usize> {
    let (l, pos) = reader.position();
    let mut yyrecord= State::new(reader);
    let mut count: isize = 0;

    // 'lex: loop {
        yyrecord.token = yyrecord.yycursor;
    /*!re2c

        [a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+
        [@]
        [a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?
        ([.][a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*
          { yyrecord.set_position(l, pos); return Some(yyrecord.yycursor); }
        * { yyrecord.set_position(l, pos); return None; }
        $ { yyrecord.set_position(l, pos); return None; }
    */ 
    // }
}

