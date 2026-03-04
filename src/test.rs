//! Testing utilities.

extern crate alloc;

use crate::parser::parse_attributes;
#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;

use crate::text;
use crate::util::is_blank;
use crate::Error;
use crate::MarkdownToHtml;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::util::{visualize_spaces, HashMap};
use crate::Result;

const ATTRIBUTE_SEPARATOR: &str = "//- - - - - - - - -//";
const CASE_SEPARATOR: &str = "//= = = = = = = = = = = = = = = = = = = = = = = =//";

/// Options for Markdown test cases.
#[derive(Debug, Clone, Copy, Default)]
pub struct MarkdownTestCaseOptions {
    pub enable_escape: bool,
    pub trim: bool,
}

/// A Markdown test case.
#[derive(Debug, Clone)]
pub struct MarkdownTestCase {
    no: u64,
    description: String,
    markdown: String,
    expected: String,
    options: MarkdownTestCaseOptions,
}

impl MarkdownTestCase {
    /// Creates a new Markdown test case.
    pub fn new(
        no: u64,
        description: String,
        markdown: String,
        expected: String,
        options: MarkdownTestCaseOptions,
    ) -> Self {
        Self {
            no,
            description,
            markdown,
            expected,
            options,
        }
    }

    /// Returns the test case number.
    pub fn no(&self) -> u64 {
        self.no
    }

    /// Returns the test case description.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the Markdown input.
    pub fn markdown(&self) -> String {
        let mut out = self.markdown.clone();
        if self.options.trim {
            out = out.trim().to_string();
        }
        if self.options.enable_escape {
            out = String::from_utf8_lossy(&apply_escape_sequence(out.as_bytes())).to_string();
        }
        out
    }

    /// Returns the expected output.
    pub fn expected(&self) -> String {
        let mut out = self.expected.clone();
        if self.options.trim {
            out = out.trim().to_string();
        }
        if self.options.enable_escape {
            out = String::from_utf8_lossy(&apply_escape_sequence(out.as_bytes())).to_string();
        }
        out
    }

    /// Executes the test case.
    pub fn execute(&self, markdown_to_html: &impl MarkdownToHtml<String>) {
        let input = self.markdown();
        let expected = self.expected();

        let mut output = String::new();
        match markdown_to_html.markdown_to_html(&mut output, &input) {
            Ok(_) => {
                if output != expected {
                    let diff = diff_pretty(&expected, &output);
                    println!(
                        r#"
============= case {}: {} ================
Markdown:
-----------
{}

Expected:
----------
{}

Actual
---------
{}

Diff
---------
{}
"#,
                        self.no,
                        self.description,
                        self.markdown(),
                        self.expected(),
                        output,
                        diff
                    );
                    panic!("\n\nTest case {} failed", self.no);
                }
            }
            Err(e) => {
                println!("Test case {} execution error: {:?}", self.no, e);
            }
        }
    }
}

/// A suite of Markdown test cases.
pub struct MarkdownTestSuite {
    cases: Vec<MarkdownTestCase>,
}

impl MarkdownTestSuite {
    /// Create a new Markdown test suite.
    ///
    /// # Panics
    /// Panics if there are duplicate test case numbers.
    pub fn new(cases: Vec<MarkdownTestCase>) -> Self {
        let mut case_nos: HashMap<u64, usize> = HashMap::new();
        for case in cases.iter() {
            if case_nos.get(&case.no()).is_some() {
                panic!("duplicate test case number {}", case.no());
            }
            case_nos.insert(case.no(), 0);
        }

        Self { cases }
    }

    /// Creates a new Markdown test cases from the given file.
    pub fn from_file(file: &str) -> Result<Self> {
        let Ok(content) = read_file(file) else {
            return Err(Error::io(
                format!("failed to read test cases from file: {}", file),
                None,
            ));
        };
        let mut cases: Vec<MarkdownTestCase> = Vec::new();
        let raw_cases: Vec<&str> = content.split(CASE_SEPARATOR).collect();

        for (i, raw_case) in raw_cases.iter().enumerate() {
            if is_blank(raw_case.as_bytes()) {
                break;
            }
            let parts: Vec<&str> = raw_case.split(ATTRIBUTE_SEPARATOR).collect();
            if parts.len() != 3 {
                return Err(Error::io(
                    format!("invalid test case format at case {}", i + 1),
                    None,
                ));
            }

            let header = parts[0].trim().to_string();
            // header format:
            //
            // no: description
            // <optional attrs>
            let mut options = MarkdownTestCaseOptions::default();
            let no = header
                .lines()
                .next()
                .and_then(|line| line.split(':').next())
                .and_then(|no_str| no_str.trim().parse::<u64>().ok())
                .unwrap_or((i + 1) as u64);
            let description = header
                .lines()
                .next()
                .and_then(|line| line.split(':').nth(1))
                .map(|s| s.trim().to_string())
                .unwrap_or(format!("Case {}", no));
            if header.lines().count() > 1 {
                let attr_line = header.lines().nth(1).unwrap();
                if let Some(attr_line) = attr_line.strip_prefix("options: ") {
                    let mut reader = text::BasicReader::new(attr_line);
                    if let Some(attrs) = parse_attributes(&mut reader) {
                        if let Some(enable_escape) = attrs.get("enableEscape") {
                            if enable_escape.str(attr_line).to_lowercase() == "true" {
                                options.enable_escape = true;
                            }
                        }
                        if let Some(trim) = attrs.get("trim") {
                            if trim.str(attr_line).to_lowercase() == "true" {
                                options.trim = true;
                            }
                        }
                    }
                }
            }

            let mut markdown = parts[1].trim_matches('\n').to_string();
            markdown.push('\n');
            let mut expected = parts[2].trim_matches('\n').to_string();
            expected.push('\n');

            let case =
                MarkdownTestCase::new((i + 1) as u64, description, markdown, expected, options);

            cases.push(case);
        }

        Ok(Self::new(cases))
    }

    /// Returns an iterator over the test cases.
    pub fn iter(&self) -> core::slice::Iter<'_, MarkdownTestCase> {
        self.cases.iter()
    }

    /// Executes all test cases.
    pub fn execute(&self, markdown_to_html: &impl MarkdownToHtml<String>) {
        println!("");
        let target_cases = parse_case_env();
        for case in &self.cases {
            if target_cases.is_empty() || target_cases.contains(&case.no()) {
                case.execute(markdown_to_html);
                println!("Test case {} passed", case.no());
            }
        }
    }
}

/// Parses the `CASE_NO` environment variable like `CASE_NO=1,2,5` and returns the case numbers.
pub fn parse_case_env() -> Vec<u64> {
    let mut case_nos: Vec<u64> = Vec::new();
    if let Some(case_no_str) = get_env("CASE_NO") {
        for part in case_no_str.split(',') {
            if let Ok(no) = part.trim().parse::<u64>() {
                case_nos.push(no);
            }
        }
    }
    case_nos
}

// diff {{{

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffType {
    Removed,
    Added,
    None,
}

/// Equivalent of Go's `diff` struct.
#[derive(Debug, Clone)]
struct Diff {
    ty: DiffType,
    lines: Vec<String>,
}

fn simple_diff(v1: &str, v2: &str) -> Vec<Diff> {
    simple_diff_aux(split_lines(v1), split_lines(v2))
}

fn split_lines(v: &str) -> Vec<String> {
    v.split('\n').map(|s| s.to_string()).collect()
}

fn simple_diff_aux(v1lines: Vec<String>, v2lines: Vec<String>) -> Vec<Diff> {
    // Index v1 lines by content (string) -> positions
    let mut v1index: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, line) in v1lines.iter().enumerate() {
        v1index.entry(line.clone()).or_default().push(i);
    }

    // Longest common substring (by lines) dynamic programming with sparse map
    let mut overlap: HashMap<usize, usize> = HashMap::new();
    let mut v1start: usize = 0;
    let mut v2start: usize = 0;
    let mut length: usize = 0;

    for (v2pos, line) in v2lines.iter().enumerate() {
        let mut new_overlap: HashMap<usize, usize> = HashMap::new();

        if let Some(v1_positions) = v1index.get(line) {
            for &v1pos in v1_positions {
                let prev = if v1pos != 0 {
                    *overlap.get(&(v1pos - 1)).unwrap_or(&0)
                } else {
                    0
                };
                let cur = prev + 1;
                new_overlap.insert(v1pos, cur);

                if cur > length {
                    length = cur;
                    v1start = v1pos + 1 - length;
                    v2start = v2pos + 1 - length;
                }
            }
        }

        overlap = new_overlap;
    }

    if length == 0 {
        let mut diffs = Vec::new();
        if !v1lines.is_empty() {
            diffs.push(Diff {
                ty: DiffType::Removed,
                lines: v1lines,
            });
        }
        if !v2lines.is_empty() {
            diffs.push(Diff {
                ty: DiffType::Added,
                lines: v2lines,
            });
        }
        return diffs;
    }

    let mut diffs = simple_diff_aux(v1lines[..v1start].to_vec(), v2lines[..v2start].to_vec());

    diffs.push(Diff {
        ty: DiffType::None,
        lines: v2lines[v2start..v2start + length].to_vec(),
    });

    diffs.extend(simple_diff_aux(
        v1lines[v1start + length..].to_vec(),
        v2lines[v2start + length..].to_vec(),
    ));

    diffs
}

/// Returns pretty formatted diff between given strings.
pub fn diff_pretty(v1: &str, v2: &str) -> String {
    let diffs = simple_diff(v1, v2);
    let mut out: String = String::new();

    for d in diffs {
        let c = match d.ty {
            DiffType::Added => '+',
            DiffType::Removed => '-',
            DiffType::None => ' ',
        };

        for line in d.lines {
            out.push(c);
            out.push_str(" | ");
            if c != ' ' {
                // assuming visualize_spaces accepts &[u8] and returns String
                out.push_str(&visualize_spaces(line.as_bytes()));
            } else {
                out.push_str(&line);
            }
            out.push('\n');
        }
    }

    out
}

// }}} diff

// utils {{{

#[allow(unreachable_code)]
fn read_file(path: &str) -> Result<String> {
    #[cfg(feature = "no-std-unix-debug")]
    {
        extern crate libc;

        let mut c_path = path.as_bytes().to_vec();
        c_path.push(0);
        let c_path_ptr = c_path.as_ptr() as *const libc::c_char;

        unsafe {
            let fd = libc::open(c_path_ptr, libc::O_RDONLY);
            if fd < 0 {
                return Err(Error::io(format!("Failed to open file: {}", path), None));
            }

            let mut out: Vec<u8> = Vec::new();
            let mut buf = [0u8; 8192];

            loop {
                let n = libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len());
                if n < 0 {
                    let _ = libc::close(fd);
                    return Err(Error::io(format!("Failed to read file: {}", path), None));
                }
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n as usize]);
            }

            if libc::close(fd) < 0 {
                return Err(Error::io(format!("Failed to close file: {}", path), None));
            }

            return Ok(String::from_utf8_lossy(&out).to_string());
        }
    }

    #[cfg(feature = "std")]
    {
        use std::fs;
        return fs::read_to_string(path)
            .map_err(|e| Error::io(format!("Failed to read file: {}", path), Some(Box::new(e))));
    }

    panic!("read_file is not implemented for no-std without std feature");
}

#[allow(unreachable_code)]
fn get_env(key: &str) -> Option<String> {
    #[cfg(feature = "no-std-unix-debug")]
    unsafe {
        extern crate libc;
        use core::slice;
        use libc::c_char;

        let mut buf = key.as_bytes().to_vec();
        buf.push(0);
        let ptr: *const c_char = buf.as_ptr() as *const c_char;

        let val = libc::getenv(ptr);
        if val.is_null() {
            return None;
        }

        let mut len: usize = 0;
        while *val.add(len) != (0 as libc::c_char) {
            len += 1;
        }

        let ret = String::from_utf8_lossy(slice::from_raw_parts(val as *const u8, len)).to_string();
        return Some(ret);
    }

    #[cfg(feature = "std")]
    {
        return std::env::var(key).ok();
    }

    panic!("get_env is not implemented for no-std without std feature");
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn apply_escape_sequence(b: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(b.len());
    let mut i = 0;

    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            match b[i + 1] {
                b'a' => {
                    result.push(0x07);
                    i += 2;
                    continue;
                }
                b'b' => {
                    result.push(0x08);
                    i += 2;
                    continue;
                }
                b'f' => {
                    result.push(0x0c);
                    i += 2;
                    continue;
                }
                b'n' => {
                    result.push(b'\n');
                    i += 2;
                    continue;
                }
                b'r' => {
                    result.push(b'\r');
                    i += 2;
                    continue;
                }
                b't' => {
                    result.push(b'\t');
                    i += 2;
                    continue;
                }
                b'v' => {
                    result.push(0x0b);
                    i += 2;
                    continue;
                }
                b'\\' => {
                    result.push(b'\\');
                    i += 2;
                    continue;
                }
                b'x' => {
                    // Go code: if len(b) >= i+3 ... (note: should be i+3 < len for two digits)
                    if i + 3 < b.len()
                        && b[i + 2].is_ascii_hexdigit()
                        && b[i + 3].is_ascii_hexdigit()
                    {
                        let hi = hex_val(b[i + 2]).unwrap();
                        let lo = hex_val(b[i + 3]).unwrap();
                        result.push((hi << 4) | lo);
                        i += 4;
                        continue;
                    }
                }
                b'u' | b'U' => {
                    if i + 2 < b.len() {
                        // collect following hex digits
                        let mut j = i + 2;
                        while j < b.len() && b[j].is_ascii_hexdigit() {
                            j += 1;
                        }
                        let num = &b[i + 2..j];

                        if num.len() >= 4 && num.len() < 8 {
                            if let Ok(s) = core::str::from_utf8(&num[..4]) {
                                if let Ok(v) = u32::from_str_radix(s, 16) {
                                    if let Some(ch) = char::from_u32(v) {
                                        let mut buf = [0u8; 4];
                                        let enc = ch.encode_utf8(&mut buf);
                                        result.extend_from_slice(enc.as_bytes());
                                        // Match the Go code's index bump (i += 5 from '\' position)
                                        i += 6;
                                        continue;
                                    }
                                }
                            }
                        }

                        if num.len() >= 8 {
                            if let Ok(s) = core::str::from_utf8(&num[..8]) {
                                if let Ok(v) = u32::from_str_radix(s, 16) {
                                    if let Some(ch) = char::from_u32(v) {
                                        let mut buf = [0u8; 4];
                                        let enc = ch.encode_utf8(&mut buf);
                                        result.extend_from_slice(enc.as_bytes());
                                        // Match the Go code's index bump (i += 9 from '\' position)
                                        i += 10;
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        result.push(b[i]);
        i += 1;
    }

    result
}

// }}} utils
