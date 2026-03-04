/*!re2c
  re2c:api = generic;
  re2c:YYCTYPE = u8;
  re2c:eof = 255;
  re2c:YYPEEK       = "if cursor < limit { *buffer.get_unchecked(cursor) } else { 255}";
  re2c:YYSKIP       = "cursor += 1;";
  re2c:YYBACKUP     = "marker = cursor;";
  re2c:YYRESTORE    = "cursor = marker;";
  re2c:YYBACKUPCTX  = "ctxmarker = cursor;";
  re2c:YYRESTORECTX = "cursor = ctxmarker;";
  re2c:YYRESTORETAG = "cursor = @@{tag};";
  re2c:YYLESSTHAN   = "limit - cursor < @@{len}";
  re2c:YYEND        = "limit == cursor";
  re2c:YYSTAGP      = "@@{tag} = cursor;";
  re2c:YYSTAGN      = "@@{tag} = 255;";
  re2c:YYSHIFT      = "cursor = (cursor as isize + @@{shift}) as usize;";
  re2c:YYSHIFTSTAG  = "@@{tag} = (@@{tag} as isize + @@{shift}) as usize;";
  re2c:yyfill:enable = 0;
*/

#[allow(warnings)] 
pub(crate) fn scan_email(buffer: &[u8]) -> Option<usize> {
   let mut cursor = 0usize;
   let limit = buffer.len();
   let mut marker = 0usize;
   let mut ctxmarker = 0usize;

    /*!re2c
        [^`][a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+
        [@]
        [a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?
        ([.][a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*
                    { return Some(cursor); }
        *           { return None; }
        $           { return None; }
    */
}

#[allow(warnings)] 
pub(crate) fn scan_url(buffer: &[u8]) -> Option<usize> {
   let mut cursor = 0usize;
   let limit = buffer.len();
   let mut marker = 0usize;
   let mut ctxmarker = 0usize;

    /*!re2c
        [A-Za-z][A-Za-z0-9.+-]{1,31}[:][^\x00-\x20<>]*
                    { return Some(cursor); }
        *           { return None; }
        $           { return None; }
    */
}

#[allow(warnings)] 
pub(crate) fn scan_table_delim_left(buffer: &[u8]) -> Option<usize> {
   let mut cursor = 0usize;
   let limit = buffer.len();
   let mut marker = 0usize;
   let mut ctxmarker = 0usize;

    /*!re2c
        [ \t\n]*[:][-]+[ \t\n]*
                    { return Some(cursor); }
        *           { return None; }
        $           { return None; }
    */
}

#[allow(warnings)] 
pub(crate) fn scan_table_delim_right(buffer: &[u8]) -> Option<usize> {
   let mut cursor = 0usize;
   let limit = buffer.len();
   let mut marker = 0usize;
   let mut ctxmarker = 0usize;

    /*!re2c
        [ \t\n]*[-]+[:][ \t\n]*
                    { return Some(cursor); }
        *           { return None; }
        $           { return None; }
    */
}

#[allow(warnings)] 
pub(crate) fn scan_table_delim_center(buffer: &[u8]) -> Option<usize> {
   let mut cursor = 0usize;
   let limit = buffer.len();
   let mut marker = 0usize;
   let mut ctxmarker = 0usize;

    /*!re2c
        [ \t\n]*[:][-]+[:][ \t\n]*
                    { return Some(cursor); }
        *           { return None; }
        $           { return None; }
    */
}

#[allow(warnings)] 
pub(crate) fn scan_table_delim_none(buffer: &[u8]) -> Option<usize> {
   let mut cursor = 0usize;
   let limit = buffer.len();
   let mut marker = 0usize;
   let mut ctxmarker = 0usize;

    /*!re2c
        [ \t\n]*[-]+[ \t\n]*
                    { return Some(cursor); }
        *           { return None; }
        $           { return None; }
    */
}
