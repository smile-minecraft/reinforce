//! Conservative LaTeX subset to native Typst math conversion.
//!
//! The converter intentionally emits only a small, self-generated Typst math
//! vocabulary. Any LaTeX text that cannot be mapped to a known symbol or
//! function becomes an escaped Typst string, so input can never be parsed as
//! Typst code. [`latex_to_typst`] returns `None` for input outside the subset,
//! which lets the caller fall back to a verbatim raw block.

use crate::typst_source::escape_typst_string;

#[derive(Debug, Clone)]
enum Token {
    Word(String),
    Number(String),
    Command(String),
    RawText(String),
    RawOperator(String),
    GroupStart,
    GroupEnd,
    Underscore,
    Caret,
    Align,
    RowBreak,
    Space,
    Char(char),
}

fn is_alpha(character: char) -> bool {
    character.is_ascii_alphabetic()
}

fn is_digit(character: char) -> bool {
    character.is_ascii_digit()
}

fn read_braced_raw(chars: &[char], index: &mut usize) -> Option<String> {
    while chars
        .get(*index)
        .is_some_and(|character| character.is_whitespace())
    {
        *index += 1;
    }
    if chars.get(*index) != Some(&'{') {
        return None;
    }
    *index += 1;
    let mut depth = 1usize;
    let mut raw = String::new();
    while let Some(&character) = chars.get(*index) {
        if character == '\\' {
            raw.push(character);
            if let Some(&next) = chars.get(*index + 1) {
                raw.push(next);
                *index += 2;
                continue;
            }
            return None;
        }
        match character {
            '{' => {
                depth += 1;
                raw.push(character);
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    *index += 1;
                    return Some(raw);
                }
                raw.push(character);
            }
            other => raw.push(other),
        }
        *index += 1;
    }
    None
}

fn tokenize(input: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while let Some(&character) = chars.get(index) {
        match character {
            '\\' => {
                index += 1;
                let next = *chars.get(index)?;
                index += 1;
                if is_alpha(next) {
                    let start = index - 1;
                    while chars.get(index).is_some_and(|c| is_alpha(*c)) {
                        index += 1;
                    }
                    let name: String = chars[start..index].iter().collect();
                    match name.as_str() {
                        "text" | "textrm" | "textbf" | "mbox" => {
                            tokens.push(Token::RawText(read_braced_raw(&chars, &mut index)?));
                        }
                        "operatorname" => {
                            tokens.push(Token::RawOperator(read_braced_raw(&chars, &mut index)?));
                        }
                        _ => tokens.push(Token::Command(name)),
                    }
                } else {
                    match next {
                        '\\' => tokens.push(Token::RowBreak),
                        ',' | ';' | ':' | '!' | ' ' => tokens.push(Token::Space),
                        '{' | '}' | '(' | ')' | '[' | ']' | '|' | '#' | '&' | '_' | '%' | '$' => {
                            tokens.push(Token::Char(next));
                        }
                        other => tokens.push(Token::Command(other.to_string())),
                    }
                }
            }
            '{' => {
                tokens.push(Token::GroupStart);
                index += 1;
            }
            '}' => {
                tokens.push(Token::GroupEnd);
                index += 1;
            }
            '_' => {
                tokens.push(Token::Underscore);
                index += 1;
            }
            '^' => {
                tokens.push(Token::Caret);
                index += 1;
            }
            '&' => {
                tokens.push(Token::Align);
                index += 1;
            }
            c if is_alpha(c) => {
                let start = index;
                while chars.get(index).is_some_and(|c| is_alpha(*c)) {
                    index += 1;
                }
                tokens.push(Token::Word(chars[start..index].iter().collect()));
            }
            c if is_digit(c) => {
                let start = index;
                while chars.get(index).is_some_and(|c| is_digit(*c)) {
                    index += 1;
                }
                tokens.push(Token::Number(chars[start..index].iter().collect()));
            }
            c if c.is_whitespace() => {
                tokens.push(Token::Space);
                index += 1;
            }
            other => {
                tokens.push(Token::Char(other));
                index += 1;
            }
        }
    }
    Some(tokens)
}

fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "alpha",
        "beta" => "beta",
        "gamma" => "gamma",
        "delta" => "delta",
        "epsilon" | "eps" => "epsilon",
        "varepsilon" => "varepsilon",
        "zeta" => "zeta",
        "eta" => "eta",
        "theta" => "theta",
        "vartheta" => "vartheta",
        "iota" => "iota",
        "kappa" => "kappa",
        "lambda" => "lambda",
        "mu" => "mu",
        "nu" => "nu",
        "xi" => "xi",
        "pi" => "pi",
        "rho" => "rho",
        "varrho" => "varrho",
        "sigma" => "sigma",
        "tau" => "tau",
        "upsilon" => "upsilon",
        "phi" => "phi",
        "varphi" => "varphi",
        "chi" => "chi",
        "psi" => "psi",
        "omega" => "omega",
        "Gamma" => "Gamma",
        "Delta" => "Delta",
        "Theta" => "Theta",
        "Lambda" => "Lambda",
        "Xi" => "Xi",
        "Pi" => "Pi",
        "Sigma" => "Sigma",
        "Phi" => "Phi",
        "Psi" => "Psi",
        "Omega" => "Omega",
        "times" => "times",
        "cdot" => "dot.op",
        "div" => "div",
        "pm" => "plus.minus",
        "mp" => "minus.plus",
        "le" | "leq" => "lt.eq",
        "ge" | "geq" => "gt.eq",
        "ne" | "neq" => "eq.not",
        "approx" => "approx",
        "equiv" => "eq.triple",
        "sim" => "tilde.op",
        "propto" => "prop",
        "in" => "in",
        "notin" => "in.not",
        "subset" => "subset",
        "subseteq" => "subset.eq",
        "supset" => "supset",
        "supseteq" => "supset.eq",
        "cup" => "union",
        "cap" => "inter",
        "emptyset" => "emptyset",
        "infty" => "infinity",
        "partial" => "partial",
        "nabla" => "gradient",
        "forall" => "forall",
        "exists" => "exists",
        "neg" | "lnot" => "not",
        "land" | "wedge" => "and",
        "lor" | "vee" => "or",
        "to" | "rightarrow" | "longrightarrow" => "arrow.r",
        "leftarrow" | "longleftarrow" => "arrow.l",
        "leftrightarrow" => "arrow.l.r",
        "Leftarrow" => "arrow.l.double",
        "Rightarrow" => "arrow.r.double",
        "Leftrightarrow" => "arrow.l.r.double",
        "mapsto" => "arrow.r.bar",
        "implies" => "arrow.r.double",
        "iff" => "arrow.l.r.double",
        "ldots" | "dots" | "cdots" => "dots.h",
        "vdots" => "dots.v",
        "ddots" => "dots.down",
        "angle" => "angle",
        "perp" => "perp",
        "parallel" => "parallel",
        "prime" => "prime",
        "sum" => "sum",
        "prod" => "product",
        "int" => "integral",
        "oint" => "integral.cont",
        "langle" => "chevron.l",
        "rangle" => "chevron.r",
        "lceil" => "ceil.l",
        "rceil" => "ceil.r",
        "lfloor" => "floor.l",
        "rfloor" => "floor.r",
        "vert" | "Vert" => "bar.v",
        _ => return None,
    })
}

fn operator(name: &str) -> Option<&'static str> {
    Some(match name {
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "cot" => "cot",
        "sec" => "sec",
        "csc" => "csc",
        "sinh" => "sinh",
        "cosh" => "cosh",
        "tanh" => "tanh",
        "arcsin" => "arcsin",
        "arccos" => "arccos",
        "arctan" => "arctan",
        "exp" => "exp",
        "log" => "log",
        "ln" => "ln",
        "lim" => "lim",
        "limsup" => "limsup",
        "liminf" => "liminf",
        "max" => "max",
        "min" => "min",
        "sup" => "sup",
        "inf" => "inf",
        "arg" => "arg",
        "det" => "det",
        "gcd" => "gcd",
        "hom" => "hom",
        "ker" => "ker",
        "deg" => "deg",
        "dim" => "dim",
        "mod" | "bmod" => "mod",
        "Pr" => "Pr",
        _ => return None,
    })
}

fn style_function(name: &str) -> Option<&'static str> {
    Some(match name {
        "mathrm" | "textrm" => "upright",
        "mathbf" => "bold",
        "mathcal" => "cal",
        "mathbb" => "bb",
        "hat" => "hat",
        "bar" | "overline" => "bar",
        "vec" => "vec",
        "dot" => "dot",
        "tilde" => "tilde",
        _ => return None,
    })
}

fn literal(character: char) -> String {
    if "+-=*/<>|.,:;!?'()[]&~".contains(character) {
        character.to_string()
    } else {
        escape_typst_string(&character.to_string())
    }
}

fn delimiter(character: char) -> Option<String> {
    Some(match character {
        '(' => "(".to_owned(),
        ')' => ")".to_owned(),
        '[' => "[".to_owned(),
        ']' => "]".to_owned(),
        '{' => "\\{".to_owned(),
        '}' => "\\}".to_owned(),
        '|' => "|".to_owned(),
        '.' => String::new(),
        _ => return None,
    })
}

fn named_delimiter(name: &str) -> Option<String> {
    if let Some(character) = name.chars().next() {
        if name.len() == 1 {
            return delimiter(character);
        }
    }
    match name {
        "langle" => Some("chevron.l".to_owned()),
        "rangle" => Some("chevron.r".to_owned()),
        "lceil" => Some("ceil.l".to_owned()),
        "rceil" => Some("ceil.r".to_owned()),
        "lfloor" => Some("floor.l".to_owned()),
        "rfloor" => Some("floor.r".to_owned()),
        "vert" | "Vert" => Some("bar.v".to_owned()),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Environment {
    Matrix,
    Pmatrix,
    Bmatrix,
    Vmatrix,
    Wmatrix,
    Cases,
    Array,
}

fn environment(name: &str) -> Option<Environment> {
    match name {
        "matrix" => Some(Environment::Matrix),
        "pmatrix" => Some(Environment::Pmatrix),
        "bmatrix" => Some(Environment::Bmatrix),
        "vmatrix" => Some(Environment::Vmatrix),
        "Vmatrix" => Some(Environment::Wmatrix),
        "cases" => Some(Environment::Cases),
        "array" => Some(Environment::Array),
        _ => None,
    }
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.position).cloned()
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.peek()?;
        self.position += 1;
        Some(token)
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(Token::Space)) {
            self.position += 1;
        }
    }

    fn parse_sequence(&mut self, stop_at_group: bool) -> Option<String> {
        let mut parts = Vec::new();
        loop {
            self.skip_spaces();
            match self.peek() {
                None => {
                    if stop_at_group {
                        return None;
                    }
                    return Some(parts.join(" "));
                }
                Some(Token::GroupEnd) => {
                    if !stop_at_group {
                        return None;
                    }
                    self.position += 1;
                    return Some(parts.join(" "));
                }
                Some(Token::Align) | Some(Token::RowBreak) => return None,
                Some(Token::Command(command)) if command == "end" => return None,
                _ => parts.push(self.parse_item()?),
            }
        }
    }

    fn parse_item(&mut self) -> Option<String> {
        let base = self.parse_base()?;
        let mut subscript = None;
        let mut superscript = None;
        loop {
            self.skip_spaces();
            match self.peek() {
                Some(Token::Underscore) => {
                    if subscript.is_some() {
                        return None;
                    }
                    self.position += 1;
                    subscript = Some(self.parse_attachment()?);
                }
                Some(Token::Caret) => {
                    if superscript.is_some() {
                        return None;
                    }
                    self.position += 1;
                    superscript = Some(self.parse_attachment()?);
                }
                _ => break,
            }
        }
        Some(match (subscript, superscript) {
            (Some(sub), Some(sup)) => format!("{base}_({sub})^({sup})"),
            (Some(sub), None) => format!("{base}_({sub})"),
            (None, Some(sup)) => format!("{base}^({sup})"),
            (None, None) => base,
        })
    }

    fn parse_attachment(&mut self) -> Option<String> {
        self.skip_spaces();
        if matches!(self.peek(), Some(Token::GroupStart)) {
            self.position += 1;
            let inner = self.parse_sequence(true)?;
            if inner.is_empty() {
                return None;
            }
            return Some(inner);
        }
        if matches!(
            self.peek(),
            None | Some(Token::GroupEnd)
                | Some(Token::Align)
                | Some(Token::RowBreak)
                | Some(Token::Underscore)
                | Some(Token::Caret)
        ) {
            return None;
        }
        self.parse_base()
    }

    fn parse_base(&mut self) -> Option<String> {
        match self.next()? {
            Token::Word(word) => {
                if word.len() == 1 && is_alpha(word.chars().next()?) {
                    Some(word)
                } else {
                    Some(escape_typst_string(&word))
                }
            }
            Token::Number(number) => Some(number),
            Token::RawText(raw) => Some(escape_typst_string(&raw)),
            Token::RawOperator(raw) => Some(format!("op({})", escape_typst_string(&raw))),
            Token::Command(command) => self.parse_command(&command),
            Token::GroupStart => {
                let inner = self.parse_sequence(true)?;
                if inner.is_empty() {
                    Some(String::new())
                } else {
                    Some(format!("({inner})"))
                }
            }
            Token::Char(character) => Some(literal(character)),
            Token::Space
            | Token::GroupEnd
            | Token::Align
            | Token::RowBreak
            | Token::Underscore
            | Token::Caret => None,
        }
    }

    fn parse_command_arg(&mut self) -> Option<String> {
        self.skip_spaces();
        if matches!(self.peek(), Some(Token::GroupStart)) {
            self.position += 1;
            let inner = self.parse_sequence(true)?;
            return if inner.is_empty() { None } else { Some(inner) };
        }
        if matches!(
            self.peek(),
            None | Some(Token::GroupEnd)
                | Some(Token::Align)
                | Some(Token::RowBreak)
                | Some(Token::Underscore)
                | Some(Token::Caret)
                | Some(Token::Space)
        ) {
            return None;
        }
        self.parse_base()
    }

    fn parse_bracket_argument(&mut self) -> Option<Option<String>> {
        self.skip_spaces();
        if !matches!(self.peek(), Some(Token::Char('['))) {
            return Some(None);
        }
        self.position += 1;
        let mut depth = 1usize;
        let mut inner = Vec::new();
        while let Some(token) = self.next() {
            match token {
                Token::Char('[') => {
                    depth += 1;
                    inner.push(Token::Char('['));
                }
                Token::Char(']') => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    inner.push(Token::Char(']'));
                }
                other => inner.push(other),
            }
        }
        if depth != 0 {
            return None;
        }
        let content = Parser::new(inner).parse_sequence(false)?;
        if content.is_empty() {
            None
        } else {
            Some(Some(content))
        }
    }

    fn parse_braced_word(&mut self) -> Option<String> {
        self.skip_spaces();
        if !matches!(self.peek(), Some(Token::GroupStart)) {
            return None;
        }
        self.position += 1;
        let name = match self.next()? {
            Token::Word(word) => word,
            _ => return None,
        };
        if !matches!(self.peek(), Some(Token::GroupEnd)) {
            return None;
        }
        self.position += 1;
        Some(name)
    }

    fn parse_delimiter(&mut self) -> Option<String> {
        self.skip_spaces();
        match self.next()? {
            Token::Char(character) => delimiter(character),
            Token::Command(name) => named_delimiter(&name),
            _ => None,
        }
    }

    fn parse_command(&mut self, name: &str) -> Option<String> {
        if let Some(value) = symbol(name) {
            return Some(value.to_owned());
        }
        if let Some(value) = operator(name) {
            return Some(value.to_owned());
        }
        if let Some(function) = style_function(name) {
            let body = self.parse_command_arg()?;
            return Some(format!("{function}({body})"));
        }
        match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let numerator = self.parse_command_arg()?;
                let denominator = self.parse_command_arg()?;
                Some(format!("frac({numerator}, {denominator})"))
            }
            "binom" => {
                let upper = self.parse_command_arg()?;
                let lower = self.parse_command_arg()?;
                Some(format!("binom({upper}, {lower})"))
            }
            "sqrt" => {
                let index = self.parse_bracket_argument()?;
                let radicand = self.parse_command_arg()?;
                Some(match index {
                    Some(index) => format!("root({index}, {radicand})"),
                    None => format!("sqrt({radicand})"),
                })
            }
            "left" | "right" => self.parse_delimiter(),
            "begin" => {
                let name = self.parse_braced_word()?;
                self.parse_environment(&name)
            }
            " " => Some(String::new()),
            _ => Some(escape_typst_string(name)),
        }
    }

    fn parse_environment(&mut self, name: &str) -> Option<String> {
        let kind = environment(name)?;
        let mut body = Vec::new();
        let mut nested = 0usize;
        loop {
            match self.next()? {
                Token::Command(command) if command == "begin" => {
                    nested += 1;
                    body.push(Token::Command(command));
                }
                Token::Command(command) if command == "end" => {
                    if nested > 0 {
                        nested -= 1;
                        body.push(Token::Command(command));
                        continue;
                    }
                    if self.parse_braced_word()? != name {
                        return None;
                    }
                    break;
                }
                other => body.push(other),
            }
        }
        if nested != 0 {
            return None;
        }
        if kind == Environment::Array {
            skip_array_spec(&mut body);
        }
        let rows = split_rows(&body)?;
        let mut rendered_rows = Vec::with_capacity(rows.len());
        for row in rows {
            let cells = if kind == Environment::Cases {
                vec![replace_alignment(row)]
            } else {
                split_cells(row)?
            };
            let mut rendered = Vec::with_capacity(cells.len());
            for cell in cells {
                let content = Parser::new(cell).parse_sequence(false)?;
                if content.is_empty() {
                    return None;
                }
                rendered.push(content);
            }
            if rendered.is_empty() {
                return None;
            }
            rendered_rows.push(rendered.join(", "));
        }
        let cells = rendered_rows.join("; ");
        Some(match kind {
            Environment::Matrix | Environment::Array => format!("mat({cells})"),
            Environment::Pmatrix => format!("mat(delim: \"(\", {cells})"),
            Environment::Bmatrix => format!("mat(delim: \"[\", {cells})"),
            Environment::Vmatrix => format!("mat(delim: \"|\", {cells})"),
            Environment::Wmatrix => format!("mat(delim: \"||\", {cells})"),
            Environment::Cases => format!("cases({cells})"),
        })
    }
}

fn skip_array_spec(body: &mut Vec<Token>) {
    if !matches!(body.first(), Some(Token::GroupStart)) {
        return;
    }
    let mut end = None;
    for (index, token) in body.iter().enumerate() {
        if matches!(token, Token::GroupEnd) {
            end = Some(index);
            break;
        }
    }
    if let Some(end) = end {
        body.drain(0..=end);
    }
}

fn split_rows(tokens: &[Token]) -> Option<Vec<Vec<Token>>> {
    split_environment(tokens, Token::RowBreak)
}

fn split_cells(tokens: Vec<Token>) -> Option<Vec<Vec<Token>>> {
    split_environment(&tokens, Token::Align)
}

fn split_environment(tokens: &[Token], separator: Token) -> Option<Vec<Vec<Token>>> {
    let mut groups = vec![Vec::new()];
    let mut depth = 0usize;
    let mut nested = 0usize;
    for token in tokens {
        match token {
            Token::GroupStart => depth += 1,
            Token::GroupEnd => {
                depth = depth.checked_sub(1)?;
            }
            Token::Command(command) if command == "begin" => nested += 1,
            Token::Command(command) if command == "end" => {
                nested = nested.checked_sub(1)?;
            }
            _ => {}
        }
        let is_separator = depth == 0
            && nested == 0
            && matches!(
                (&separator, token),
                (Token::RowBreak, Token::RowBreak) | (Token::Align, Token::Align)
            );
        if is_separator {
            groups.push(Vec::new());
        } else {
            groups
                .last_mut()
                .expect("split_environment always starts with one group")
                .push(token.clone());
        }
    }
    if depth != 0 || nested != 0 {
        return None;
    }
    groups.retain(|group| !group.is_empty());
    if groups.is_empty() {
        None
    } else {
        Some(groups)
    }
}

fn replace_alignment(mut tokens: Vec<Token>) -> Vec<Token> {
    for token in &mut tokens {
        if matches!(token, Token::Align) {
            *token = Token::Char('&');
        }
    }
    tokens
}

fn chemistry_argument(input: &str) -> Option<String> {
    let rest = input.strip_prefix("\\ce")?.trim_start();
    let chars: Vec<char> = rest.chars().collect();
    if chars.first() != Some(&'{') {
        return None;
    }
    let mut depth = 0usize;
    for (index, character) in chars.iter().enumerate() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if index + 1 != chars.len() {
                        return None;
                    }
                    return Some(chars[1..index].iter().collect());
                }
            }
            _ => {}
        }
    }
    None
}

fn charge_boundary(next: Option<&char>) -> bool {
    match next {
        None => true,
        Some(character) => {
            character.is_whitespace()
                || matches!(character, '+' | '-' | ')' | ']' | '=' | '<' | '>')
        }
    }
}

fn append_attachment(parts: &mut [String], attachment: String) -> Option<()> {
    let last = parts.last_mut()?;
    last.push_str(&attachment);
    Some(())
}

fn parse_chemistry_group(chars: &[char], index: &mut usize) -> Option<String> {
    let open = chars[*index];
    let close = match open {
        '(' => ')',
        '[' => ']',
        _ => return None,
    };
    let start = *index + 1;
    let mut depth = 1usize;
    *index += 1;
    while *index < chars.len() {
        match chars[*index] {
            character if character == open => depth += 1,
            character if character == close => {
                depth -= 1;
                if depth == 0 {
                    let inner: String = chars[start..*index].iter().collect();
                    *index += 1;
                    return convert_chemistry(&inner);
                }
            }
            _ => {}
        }
        *index += 1;
    }
    None
}

fn parse_chemistry_superscript(chars: &[char], index: &mut usize) -> Option<String> {
    if chars.get(*index) == Some(&'{') {
        return read_braced_raw(chars, index);
    }
    let start = *index;
    while let Some(&character) = chars.get(*index) {
        if character.is_whitespace() || matches!(character, '(' | ')' | '[' | ']' | '=' | '<' | '>')
        {
            break;
        }
        if matches!(character, '+' | '-') && *index > start && !chars[*index - 1].is_ascii_digit() {
            break;
        }
        *index += 1;
    }
    if *index == start {
        None
    } else {
        Some(chars[start..*index].iter().collect())
    }
}

fn convert_chemistry(input: &str) -> Option<String> {
    let chars: Vec<char> = input.chars().collect();
    let mut parts = Vec::new();
    let mut index = 0usize;
    let mut expect_charge = false;
    while let Some(&character) = chars.get(index) {
        if character.is_whitespace() {
            index += 1;
            expect_charge = false;
            continue;
        }
        if character == '-' && chars.get(index + 1) == Some(&'>') {
            parts.push("arrow.r".to_owned());
            index += 2;
            expect_charge = false;
            continue;
        }
        if character == '<'
            && chars.get(index + 1) == Some(&'=')
            && chars.get(index + 2) == Some(&'>')
        {
            parts.push("arrow.l.r".to_owned());
            index += 3;
            expect_charge = false;
            continue;
        }
        if character == '<'
            && chars.get(index + 1) == Some(&'-')
            && chars.get(index + 2) == Some(&'>')
        {
            parts.push("arrow.l.r".to_owned());
            index += 3;
            expect_charge = false;
            continue;
        }
        if character == '<' && chars.get(index + 1) == Some(&'=') {
            parts.push("lt.eq".to_owned());
            index += 2;
            expect_charge = false;
            continue;
        }
        if character == '>' && chars.get(index + 1) == Some(&'=') {
            parts.push("gt.eq".to_owned());
            index += 2;
            expect_charge = false;
            continue;
        }
        match character {
            '+' | '-' => {
                if expect_charge && charge_boundary(chars.get(index + 1)) {
                    let escaped = escape_typst_string(&character.to_string());
                    append_attachment(&mut parts, format!("^({escaped})"))?;
                    expect_charge = false;
                } else {
                    parts.push(character.to_string());
                    expect_charge = false;
                }
                index += 1;
            }
            '^' => {
                index += 1;
                let superscript = parse_chemistry_superscript(&chars, &mut index)?;
                let escaped = escape_typst_string(&superscript);
                append_attachment(&mut parts, format!("^({escaped})"))?;
                expect_charge = false;
            }
            '(' | '[' => {
                let inner = parse_chemistry_group(&chars, &mut index)?;
                parts.push(format!("({inner})"));
                expect_charge = true;
            }
            c if is_digit(c) => {
                let start = index;
                while chars.get(index).is_some_and(|c| is_digit(*c)) {
                    index += 1;
                }
                let digits: String = chars[start..index].iter().collect();
                if expect_charge {
                    append_attachment(&mut parts, format!("_({digits})"))?;
                } else {
                    parts.push(digits);
                }
            }
            c if c.is_ascii_uppercase() => {
                let mut element = String::new();
                while let Some(&upper) = chars.get(index) {
                    if !upper.is_ascii_uppercase() {
                        break;
                    }
                    element.push(upper);
                    index += 1;
                    while let Some(&lower) = chars.get(index) {
                        if !lower.is_ascii_lowercase() {
                            break;
                        }
                        element.push(lower);
                        index += 1;
                    }
                    if !chars
                        .get(index)
                        .is_some_and(|character| character.is_ascii_uppercase())
                    {
                        break;
                    }
                }
                parts.push(format!("upright({})", escape_typst_string(&element)));
                expect_charge = true;
            }
            other => {
                parts.push(escape_typst_string(&other.to_string()));
                index += 1;
                expect_charge = false;
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn is_safe_math(expression: &str) -> bool {
    let chars: Vec<char> = expression.chars().collect();
    let mut index = 0usize;
    let mut in_string = false;
    while let Some(&character) = chars.get(index) {
        if in_string {
            if character == '\\' {
                index += 2;
                continue;
            }
            if character == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match character {
            '"' => {
                in_string = true;
                index += 1;
            }
            '\\' => {
                if matches!(chars.get(index + 1), Some('{' | '}')) {
                    index += 2;
                } else {
                    return false;
                }
            }
            '#' | '@' | '$' => return false,
            c if c.is_ascii_alphanumeric() || c.is_whitespace() => index += 1,
            c if "()[],;_^+-*/=<>|.!?&:'~%".contains(c) => index += 1,
            _ => return false,
        }
    }
    !in_string
}

/// Convert a supported LaTeX math fragment to native Typst math content.
///
/// The returned string is the expression inside `$ ... $`, not the equation
/// itself. `None` means the input is outside the supported subset and the
/// caller should render the original source verbatim.
pub fn latex_to_typst(latex: &str) -> Option<String> {
    let trimmed = latex.trim();
    if trimmed.is_empty() {
        return None;
    }
    let input = trimmed
        .strip_prefix('$')
        .and_then(|value| value.strip_suffix('$'))
        .unwrap_or(trimmed);
    let expression = if let Some(chemistry) = chemistry_argument(input) {
        convert_chemistry(&chemistry)?
    } else {
        let tokens = tokenize(input)?;
        if tokens.is_empty() {
            return None;
        }
        Parser::new(tokens).parse_sequence(false)?
    };
    if expression.is_empty() || !is_safe_math(&expression) {
        return None;
    }
    Some(expression)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_math_becomes_native_typst() {
        assert_eq!(
            latex_to_typst(r"\frac{a+b}{c}").as_deref(),
            Some("frac(a + b, c)")
        );
        assert_eq!(
            latex_to_typst(r"\sqrt[3]{x}").as_deref(),
            Some("root(3, x)")
        );
        assert_eq!(
            latex_to_typst(r"\alpha + \beta = \gamma").as_deref(),
            Some("alpha + beta = gamma")
        );
        assert_eq!(
            latex_to_typst(r"\sum_{i=1}^n i").as_deref(),
            Some("sum_(i = 1)^(n) i")
        );
        assert_eq!(
            latex_to_typst(r"\int_0^1 x dx").as_deref(),
            Some("integral_(0)^(1) x \"dx\"")
        );
        assert_eq!(
            latex_to_typst(r"\left(\frac{1}{2}\right)").as_deref(),
            Some("( frac(1, 2) )")
        );
    }

    #[test]
    fn matrices_and_text_are_converted() {
        assert_eq!(
            latex_to_typst(r"\begin{pmatrix}a & b \\ c & d\end{pmatrix}").as_deref(),
            Some("mat(delim: \"(\", a, b; c, d)")
        );
        assert_eq!(
            latex_to_typst(r"\begin{cases}x & \text{if } x > 0 \\ -x & \text{else}\end{cases}")
                .as_deref(),
            Some("cases(x & \"if \" x > 0; - x & \"else\")")
        );
        assert_eq!(
            latex_to_typst(r"\operatorname{rank}(A)").as_deref(),
            Some("op(\"rank\") ( A )")
        );
    }

    #[test]
    fn common_reactions_are_converted() {
        assert_eq!(
            latex_to_typst(r"\ce{2H2 + O2 -> 2H2O}").as_deref(),
            Some(
                "2 upright(\"H\")_(2) + upright(\"O\")_(2) arrow.r 2 upright(\"H\")_(2) upright(\"O\")"
            )
        );
        assert_eq!(
            latex_to_typst(r"\ce{H+ + OH- -> H2O}").as_deref(),
            Some(
                "upright(\"H\")^(\"+\") + upright(\"OH\")^(\"-\") arrow.r upright(\"H\")_(2) upright(\"O\")"
            )
        );
        assert_eq!(
            latex_to_typst(r"\ce{CO3^2- <=> HCO3-}").as_deref(),
            Some("upright(\"CO\")_(3)^(\"2-\") arrow.l.r upright(\"HCO\")_(3)^(\"-\")")
        );
    }

    #[test]
    fn unsupported_input_falls_back() {
        assert_eq!(latex_to_typst(r"\frac{1"), None);
        assert_eq!(latex_to_typst(r"\begin{matrix}a & b"), None);
        assert_eq!(latex_to_typst(""), None);
    }

    #[test]
    fn user_text_never_becomes_code() {
        let input = r#"\text{#panic("x")}"#;
        let converted = latex_to_typst(input).expect("text must convert");
        assert_eq!(converted, r##""#panic(\"x\")""##);
        assert!(is_safe_math(&converted));

        let unknown = latex_to_typst(r"\panic(1)").expect("unknown commands become text");
        assert_eq!(unknown, "\"panic\" ( 1 )");
        assert!(!unknown.contains('#'));
    }
}
