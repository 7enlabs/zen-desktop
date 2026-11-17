# SPEC - Zen Desktop: Rust rebuild pilot (cowsay@1.6.0 parity)
## Hard rules
- Rust stable, std only (no crates). Binary `cowsay`. Linux, macOS, Windows (x86_64 + aarch64).
- Output must byte-match spec/cases/*.out for all 11 cases. Always write "\n" (never \r\n) to stdout, including on Windows; read stdin as UTF-8 bytes, accept \r\n.
## CLI
`cowsay [-n] [-W N] [message...]`; `-W` takes next arg (also `-W40`); default 40; unknown flags error to stderr exit 2.
## Algorithm
1. text = args.join(" ") or stdin minus one trailing "\n"/"\r\n".
2. normalize \r\n,\r -> \n; \t -> 8 spaces.
3. lines: split on \n; if wrap, cut each line into chunks of W UTF-16 code units (char.len_utf16()), never splitting a surrogate pair; empty line stays one "" chunk; empty text => zero lines.
4. width(s): sum per char: 0 for combining marks (U+0300–036F, 1AB0–1AFF, 1DC0–1DFF, 20D0–20FF, FE20–FE2F) and ZWJ/VS16 (200D, FE0F); 2 for East Asian Wide/Fullwidth ranges (1100–115F, 2E80–303E, 3041–33FF, 3400–4DBF, 4E00–9FFF, A000–A4CF, AC00–D7A3, F900–FAFF, FE30–FE4F, FF00–FF60, FFE0–FFE6, 1F300–1F64F, 1F900–1F9FF, 20000–3FFFD); else 1. Strip ANSI CSI sequences before measuring.
5. max = max width; pad each line with spaces to max.
6. 1 line -> "< l >"; n>1 -> first "/ l \\", middle "| l |", last "\\ l /".
7. print " "+"_"*(max+2), lines, " "+"-"*(max+2), then the default cow (copy spec/cases/01.out tail).
## Layout
src/main.rs (CLI), src/balloon.rs (normalize/wrap/width/render), src/cow.rs, tests/golden.rs reading spec/cases.
## Roadmap (from external-tool research, opt-in flags, never change default output)
- DONE 2026-10-16 `--word-wrap`: greedy by display columns, break at spaces, collapse space runs, hard-cut words wider than W; default output unchanged.
- DONE 2026-10-16 190 cowfiles embedded (include_str!), -f name|path, -r/--random, -l (Node readdir order), modes -bdgpstwy, -e, -T, --think / cowthink argv0. Golden 21/21.
- DONE 2026-10-16 `-W auto` = $COLUMNS - 4, fallback 40 (std-only; ioctl/WinAPI later).
- DONE 2026-10-16 library: cowsay_bubble::say(text, &Options) with Wrap::{None,Hard,Words}, auto_width(); doctest.
- DONE (file only, not pushed) .github/workflows/ci.yml: ubuntu/macos/windows test + parity + artifacts.
