use cowsay_bubble::{cow, Options, Wrap};

use std::io::{IsTerminal, Read, Write};

fn usage(code: i32) -> ! {
    eprintln!("Usage: cowsay [-bdgpstwy] [-e eyes] [-T tongue] [-f cowfile] [-r|--random] [-l] [-n] [-W <width>] [--think] [--word-wrap] [--cow-color <name>] [--color=auto|always|never] [message...]\n  -n  disable wrapping\n  -W  wrap column (default 40; 'auto' = terminal width from $COLUMNS)\n  --word-wrap  break at spaces instead of hard cut (Zen extension)\n  --cow-color  red|green|yellow|blue|magenta|cyan|white|gray|bold|rainbow|aurora (Zen extension)\n  --color  when to colour: auto (tty, honours NO_COLOR/CLICOLOR_FORCE), always, never");
    std::process::exit(code)
}

fn main() {
    let argv0 = std::env::args().next().unwrap_or_default();
    let mut think = std::path::Path::new(&argv0)
        .file_stem()
        .is_some_and(|s| s.to_string_lossy().ends_with("think"));
    let (mut wrap_on, mut by_word, mut random, mut list) = (true, false, false, false);
    let mut w: usize = 40;
    let (mut eyes, mut tongue, mut file) =
        ("oo".to_string(), "  ".to_string(), "default".to_string());
    let mut mode: Option<char> = None;
    let mut paint: Option<cowsay_bubble::color::Paint> = None;
    let mut when = cowsay_bubble::color::When::Auto;
    let mut words: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    let val = |s: &str, args: &mut dyn Iterator<Item = String>| {
        if s.len() > 2 {
            s[2..].to_string()
        } else {
            args.next().unwrap_or_else(|| usage(2))
        }
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => usage(0),
            "--word-wrap" => by_word = true,
            "--think" => think = true,
            "--random" => random = true,
            "--list" => list = true,
            "--" => words.extend(args.by_ref()),
            "--cow-color" => {
                let v = args.next().unwrap_or_else(|| usage(2));
                paint = Some(cowsay_bubble::color::parse(&v).unwrap_or_else(|| usage(2)));
            }
            s if s.starts_with("--cow-color=") => {
                paint = Some(cowsay_bubble::color::parse(&s[12..]).unwrap_or_else(|| usage(2)));
            }
            "--color" => when = cowsay_bubble::color::When::Always,
            s if s.starts_with("--color=") => {
                when = cowsay_bubble::color::parse_when(&s[8..]).unwrap_or_else(|| usage(2));
            }
            s if s.starts_with("-W") => {
                let v = val(s, &mut args);
                w = if v.trim() == "auto" {
                    cowsay_bubble::auto_width()
                } else {
                    v.trim().parse().unwrap_or_else(|_| usage(2))
                };
            }
            s if s.starts_with("-e") => eyes = val(s, &mut args),
            s if s.starts_with("-T") => tongue = val(s, &mut args),
            s if s.starts_with("-f") => file = val(s, &mut args),
            s if s.starts_with('-') && s.len() > 1 && !s.starts_with("--") => {
                for c in s[1..].chars() {
                    match c {
                        'n' => wrap_on = false,
                        'r' => random = true,
                        'l' => list = true,
                        'h' => usage(0),
                        c if cow::MODES.iter().any(|m| m.0 == c) => {
                            let rank = |x: char| cow::MODES.iter().position(|m| m.0 == x).unwrap();
                            if mode.is_none_or(|m| rank(c) < rank(m)) {
                                mode = Some(c);
                            }
                        }
                        _ => usage(2),
                    }
                }
            }
            s if s.starts_with("--") => usage(2),
            _ => words.push(a),
        }
    }
    let mut out = std::io::stdout().lock();
    if list {
        writeln!(out, "{}", cow::names().join("  ")).ok();
        return;
    }
    if random {
        let n = cow::names();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()) as usize;
        file = n[(seed ^ (std::process::id() as usize).wrapping_mul(2654435761)) % n.len()]
            .to_string();
    }
    if let Some(m) = mode.and_then(|c| cow::MODES.iter().find(|x| x.0 == c)) {
        eyes = m.1.to_string();
        tongue = m.2.to_string();
    }

    let text = if !words.is_empty() {
        words.join(" ")
    } else {
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf).ok();
        let mut s = String::from_utf8_lossy(&buf).into_owned();
        if s.is_empty() {
            usage(1);
        }
        if s.ends_with("\r\n") {
            s.truncate(s.len() - 2);
        } else if s.ends_with('\n') || s.ends_with('\r') {
            s.pop();
        }
        s
    };
    let opts = Options {
        wrap: if !wrap_on {
            Wrap::None
        } else if by_word {
            Wrap::Words(w)
        } else {
            Wrap::Hard(w)
        },
        cow: file,
        eyes,
        tongue,
        think,
    };
    match cowsay_bubble::say(&text, &opts) {
        Ok(s) => {
            let env = |k: &str| std::env::var(k).ok();
            let s = match paint {
                Some(p)
                    if cowsay_bubble::color::enabled(
                        when,
                        std::io::stdout().is_terminal(),
                        env("NO_COLOR").as_deref(),
                        env("CLICOLOR_FORCE").as_deref(),
                    ) && cowsay_bubble::color::enable_vt() =>
                {
                    cowsay_bubble::color::paint_depth(
                        &s,
                        p,
                        cowsay_bubble::color::depth(env("COLORTERM").as_deref()),
                    )
                }
                _ => s,
            };
            out.write_all(s.as_bytes()).ok();
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1)
        }
    }
}
