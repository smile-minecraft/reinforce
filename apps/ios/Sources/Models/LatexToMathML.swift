import Foundation

/// Pure-Swift LaTeX-to-MathML converter for offline WebKit rendering.
/// Covers common physics, math, and chemistry (`\ce{...}`) notation without
/// bundling third-party JS/font assets or requiring network access.
enum LatexToMathML {
    static func renderMathML(latex: String, display: Bool) -> String {
        let parser = Parser(source: latex)
        let body = parser.parseSequence(stopAt: nil)
        let mode = display ? "block" : "inline"
        return "<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"\(mode)\">\(body)</math>"
    }

    private final class Parser {
        private let chars: [Character]
        private var pos: Int = 0

        init(source: String) {
            self.chars = Array(source)
        }

        private var isAtEnd: Bool { pos >= chars.count }

        private func peek() -> Character? {
            isAtEnd ? nil : chars[pos]
        }

        private func advance() -> Character? {
            guard !isAtEnd else { return nil }
            defer { pos += 1 }
            return chars[pos]
        }

        private func skipWhitespace() {
            while let ch = peek(), ch.isWhitespace {
                pos += 1
            }
        }

        func parseSequence(stopAt: Character?) -> String {
            var pieces: [String] = []
            while let ch = peek() {
                if let stopAt, ch == stopAt {
                    pos += 1
                    break
                }
                if ch.isWhitespace {
                    pos += 1
                    continue
                }
                if let atom = parseAtomWithScripts() {
                    pieces.append(atom)
                }
            }
            return pieces.joined()
        }

        private func parseGroupOrAtom() -> String {
            skipWhitespace()
            guard let ch = peek() else { return "<mrow></mrow>" }
            if ch == "{" {
                pos += 1
                let inner = parseSequence(stopAt: "}")
                return "<mrow>\(inner)</mrow>"
            }
            return parseBaseAtom() ?? "<mrow></mrow>"
        }

        private func parseAtomWithScripts() -> String? {
            guard var base = parseBaseAtom() else { return nil }

            var subscriptPart: String?
            var superscriptPart: String?

            while true {
                skipWhitespace()
                guard let next = peek() else { break }
                if next == "_" && subscriptPart == nil {
                    pos += 1
                    subscriptPart = parseGroupOrAtom()
                } else if next == "^" && superscriptPart == nil {
                    pos += 1
                    superscriptPart = parseGroupOrAtom()
                } else {
                    break
                }
            }

            if let sub = subscriptPart, let sup = superscriptPart {
                base = "<msubsup>\(base)\(sub)\(sup)</msubsup>"
            } else if let sub = subscriptPart {
                base = "<msub>\(base)\(sub)</msub>"
            } else if let sup = superscriptPart {
                base = "<msup>\(base)\(sup)</msup>"
            }
            return base
        }

        private func parseBaseAtom() -> String? {
            skipWhitespace()
            guard let ch = advance() else { return nil }

            if ch == "{" {
                let inner = parseSequence(stopAt: "}")
                return "<mrow>\(inner)</mrow>"
            }

            if ch == "\\" {
                return parseCommand()
            }

            if ch.isNumber {
                var num = String(ch)
                while let next = peek(), next.isNumber || next == "." {
                    num.append(next)
                    pos += 1
                }
                return "<mn>\(escape(num))</mn>"
            }

            if ch.isLetter {
                return "<mi>\(escape(String(ch)))</mi>"
            }

            if ch == "'" {
                return "<mo>&#x2032;</mo>"
            }

            return "<mo>\(escape(String(ch)))</mo>"
        }

        private func parseCommand() -> String {
            guard let first = peek() else { return "" }
            if !first.isLetter {
                pos += 1
                switch first {
                case ",", ";", ":", " ":
                    return "<mspace width=\"0.2em\"/>"
                case "!":
                    return ""
                case "{", "}", "%", "#", "&", "$", "_":
                    return "<mo>\(escape(String(first)))</mo>"
                case "\\":
                    return "<mspace linebreak=\"newline\"/>"
                default:
                    return "<mo>\(escape(String(first)))</mo>"
                }
            }

            var name = ""
            while let ch = peek(), ch.isLetter {
                name.append(ch)
                pos += 1
            }

            switch name {
            case "frac", "dfrac", "tfrac":
                let num = parseGroupOrAtom()
                let den = parseGroupOrAtom()
                return "<mfrac>\(num)\(den)</mfrac>"
            case "sqrt":
                skipWhitespace()
                if peek() == "[" {
                    pos += 1
                    let degree = parseSequence(stopAt: "]")
                    let radicand = parseGroupOrAtom()
                    return "<mroot>\(radicand)<mrow>\(degree)</mrow></mroot>"
                }
                let radicand = parseGroupOrAtom()
                return "<msqrt>\(radicand)</msqrt>"
            case "text", "textrm", "mathrm", "operatorname":
                let text = readRawGroup()
                return "<mtext>\(escape(text))</mtext>"
            case "mathbf", "boldsymbol":
                let inner = parseGroupOrAtom()
                return "<mstyle mathvariant=\"bold\">\(inner)</mstyle>"
            case "ce":
                let formula = readRawGroup()
                return renderChemistry(formula)
            case "left", "right":
                skipWhitespace()
                guard let delim = advance() else { return "" }
                if delim == "." { return "" }
                if delim == "\\" {
                    let sym = parseCommand()
                    return sym
                }
                return "<mo stretchy=\"true\">\(escape(String(delim)))</mo>"
            case "vec":
                let inner = parseGroupOrAtom()
                return "<mover>\(inner)<mo>&#x2192;</mo></mover>"
            case "bar", "overline":
                let inner = parseGroupOrAtom()
                return "<mover>\(inner)<mo>&#x00AF;</mo></mover>"
            case "hat":
                let inner = parseGroupOrAtom()
                return "<mover>\(inner)<mo>^</mo></mover>"
            case "dot":
                let inner = parseGroupOrAtom()
                return "<mover>\(inner)<mo>&#x02D9;</mo></mover>"
            default:
                if let symbol = Self.symbolTable[name] {
                    let tag = symbol.isIdentifier ? "mi" : "mo"
                    return "<\(tag)>\(symbol.glyph)</\(tag)>"
                }
                if Self.functionNames.contains(name) {
                    return "<mi mathvariant=\"normal\">\(escape(name))</mi>"
                }
                return "<mi>\(escape(name))</mi>"
            }
        }

        private func readRawGroup() -> String {
            skipWhitespace()
            guard peek() == "{" else {
                return advance().map(String.init) ?? ""
            }
            pos += 1
            var depth = 1
            var result = ""
            while let ch = advance() {
                if ch == "{" {
                    depth += 1
                    result.append(ch)
                } else if ch == "}" {
                    depth -= 1
                    if depth == 0 { break }
                    result.append(ch)
                } else {
                    result.append(ch)
                }
            }
            return result
        }

        private func renderChemistry(_ raw: String) -> String {
            let replaced = raw
                .replacingOccurrences(of: "<=>", with: " \u{21CC} ")
                .replacingOccurrences(of: "->", with: " \u{2192} ")
            var out = "<mrow>"
            let chars = Array(replaced)
            var i = 0
            while i < chars.count {
                let ch = chars[i]
                if ch.isNumber && i > 0 && (chars[i - 1].isLetter || chars[i - 1] == ")") {
                    var digits = ""
                    while i < chars.count && chars[i].isNumber {
                        digits.append(chars[i])
                        i += 1
                    }
                    out += "<msub><mrow></mrow><mn>\(escape(digits))</mn></msub>"
                    continue
                }
                if ch.isLetter {
                    var letters = ""
                    while i < chars.count && chars[i].isLetter {
                        letters.append(chars[i])
                        i += 1
                    }
                    out += "<mi mathvariant=\"normal\">\(escape(letters))</mi>"
                    continue
                }
                if ch.isWhitespace {
                    out += "<mspace width=\"0.2em\"/>"
                } else {
                    out += "<mo>\(escape(String(ch)))</mo>"
                }
                i += 1
            }
            out += "</mrow>"
            return out
        }

        private func escape(_ text: String) -> String {
            text
                .replacingOccurrences(of: "&", with: "&amp;")
                .replacingOccurrences(of: "<", with: "&lt;")
                .replacingOccurrences(of: ">", with: "&gt;")
        }

        private struct SymbolEntry {
            let glyph: String
            let isIdentifier: Bool
        }

        private static let functionNames: Set<String> = [
            "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan",
            "sinh", "cosh", "tanh", "log", "ln", "exp", "lim", "min", "max", "sup", "inf", "det", "deg"
        ]

        private static let symbolTable: [String: SymbolEntry] = [
            // Greek uppercase
            "Gamma": .init(glyph: "\u{0393}", isIdentifier: true),
            "Delta": .init(glyph: "\u{0394}", isIdentifier: true),
            "Theta": .init(glyph: "\u{0398}", isIdentifier: true),
            "Lambda": .init(glyph: "\u{039B}", isIdentifier: true),
            "Xi": .init(glyph: "\u{039E}", isIdentifier: true),
            "Pi": .init(glyph: "\u{03A0}", isIdentifier: true),
            "Sigma": .init(glyph: "\u{03A3}", isIdentifier: true),
            "Phi": .init(glyph: "\u{03A6}", isIdentifier: true),
            "Psi": .init(glyph: "\u{03A8}", isIdentifier: true),
            "Omega": .init(glyph: "\u{03A9}", isIdentifier: true),
            // Greek lowercase
            "alpha": .init(glyph: "\u{03B1}", isIdentifier: true),
            "beta": .init(glyph: "\u{03B2}", isIdentifier: true),
            "gamma": .init(glyph: "\u{03B3}", isIdentifier: true),
            "delta": .init(glyph: "\u{03B4}", isIdentifier: true),
            "epsilon": .init(glyph: "\u{03B5}", isIdentifier: true),
            "varepsilon": .init(glyph: "\u{03B5}", isIdentifier: true),
            "zeta": .init(glyph: "\u{03B6}", isIdentifier: true),
            "eta": .init(glyph: "\u{03B7}", isIdentifier: true),
            "theta": .init(glyph: "\u{03B8}", isIdentifier: true),
            "kappa": .init(glyph: "\u{03BA}", isIdentifier: true),
            "lambda": .init(glyph: "\u{03BB}", isIdentifier: true),
            "mu": .init(glyph: "\u{03BC}", isIdentifier: true),
            "nu": .init(glyph: "\u{03BD}", isIdentifier: true),
            "xi": .init(glyph: "\u{03BE}", isIdentifier: true),
            "pi": .init(glyph: "\u{03C0}", isIdentifier: true),
            "rho": .init(glyph: "\u{03C1}", isIdentifier: true),
            "sigma": .init(glyph: "\u{03C3}", isIdentifier: true),
            "tau": .init(glyph: "\u{03C4}", isIdentifier: true),
            "phi": .init(glyph: "\u{03C6}", isIdentifier: true),
            "varphi": .init(glyph: "\u{03C6}", isIdentifier: true),
            "chi": .init(glyph: "\u{03C7}", isIdentifier: true),
            "psi": .init(glyph: "\u{03C8}", isIdentifier: true),
            "omega": .init(glyph: "\u{03C9}", isIdentifier: true),
            // Operators & relations
            "cdot": .init(glyph: "\u{00B7}", isIdentifier: false),
            "times": .init(glyph: "\u{00D7}", isIdentifier: false),
            "div": .init(glyph: "\u{00F7}", isIdentifier: false),
            "pm": .init(glyph: "\u{00B1}", isIdentifier: false),
            "mp": .init(glyph: "\u{2213}", isIdentifier: false),
            "leq": .init(glyph: "\u{2264}", isIdentifier: false),
            "le": .init(glyph: "\u{2264}", isIdentifier: false),
            "geq": .init(glyph: "\u{2265}", isIdentifier: false),
            "ge": .init(glyph: "\u{2265}", isIdentifier: false),
            "neq": .init(glyph: "\u{2260}", isIdentifier: false),
            "ne": .init(glyph: "\u{2260}", isIdentifier: false),
            "approx": .init(glyph: "\u{2248}", isIdentifier: false),
            "equiv": .init(glyph: "\u{2261}", isIdentifier: false),
            "propto": .init(glyph: "\u{221D}", isIdentifier: false),
            "infty": .init(glyph: "\u{221E}", isIdentifier: true),
            "partial": .init(glyph: "\u{2202}", isIdentifier: false),
            "nabla": .init(glyph: "\u{2207}", isIdentifier: false),
            "sum": .init(glyph: "\u{2211}", isIdentifier: false),
            "prod": .init(glyph: "\u{220F}", isIdentifier: false),
            "int": .init(glyph: "\u{222B}", isIdentifier: false),
            "oint": .init(glyph: "\u{222E}", isIdentifier: false),
            "to": .init(glyph: "\u{2192}", isIdentifier: false),
            "rightarrow": .init(glyph: "\u{2192}", isIdentifier: false),
            "leftarrow": .init(glyph: "\u{2190}", isIdentifier: false),
            "leftrightarrow": .init(glyph: "\u{2194}", isIdentifier: false),
            "Rightarrow": .init(glyph: "\u{21D2}", isIdentifier: false),
            "rightleftharpoons": .init(glyph: "\u{21CC}", isIdentifier: false),
            "degree": .init(glyph: "\u{00B0}", isIdentifier: false),
            "circ": .init(glyph: "\u{2218}", isIdentifier: false),
            "in": .init(glyph: "\u{2208}", isIdentifier: false),
            "notin": .init(glyph: "\u{2209}", isIdentifier: false),
            "subset": .init(glyph: "\u{2282}", isIdentifier: false),
            "subseteq": .init(glyph: "\u{2286}", isIdentifier: false),
            "cup": .init(glyph: "\u{222A}", isIdentifier: false),
            "cap": .init(glyph: "\u{2229}", isIdentifier: false),
            "forall": .init(glyph: "\u{2200}", isIdentifier: false),
            "exists": .init(glyph: "\u{2203}", isIdentifier: false),
            "dots": .init(glyph: "\u{2026}", isIdentifier: false),
            "cdots": .init(glyph: "\u{22EF}", isIdentifier: false),
        ]
    }
}
