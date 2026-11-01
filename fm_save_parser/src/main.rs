use fm_save_parser::container::FmContainer;
use fm_save_parser::error::{FmError, Result};
use fm_save_parser::version;
use std::path::Path;
use std::time::Instant;

const USAGE: &str = "\
fm_save_parser - Football Manager save parser

USAGE:
  fm_save_parser info <save.fm>                 version + container summary
  fm_save_parser ls <save.fm>                   list files inside the save
  fm_save_parser extract <save.fm> <entry> <out> decompress one file (e.g. game_db.dat)
  fm_save_parser players <save.fm> [--top N] [--min-ca N] [--json <out.json>]
                         [--nation-names <uid,name file>] [--force-probe]
                                                extract players (top N by CA printed)
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let res = match args.first().map(String::as_str) {
        Some("info") if args.len() == 2 => cmd_info(&args[1]),
        Some("ls") if args.len() == 2 => cmd_ls(&args[1]),
        Some("extract") if args.len() == 4 => cmd_extract(&args[1], &args[2], &args[3]),
        Some("players") if args.len() >= 2 => cmd_players(&args[1], &args[2..]),
        _ => {
            eprint!("{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn cmd_info(path: &str) -> Result<()> {
    let c = FmContainer::open(Path::new(path))?;
    let v = version::detect(&c)?;
    let total: u64 = c.entries.iter().map(|e| e.uncompressed_size).sum();
    println!("title:          {}", c.title);
    println!("file size:      {} bytes", c.len());
    println!("game version:   {} (created), last saved with {}", v.game_version.as_deref().unwrap_or("?"), v.saved_with.as_deref().unwrap_or("?"));
    println!("in-game date:   {}", v.current_date.as_deref().unwrap_or("?"));
    println!("major:          FM{}", v.major);
    if v.layout_major() != v.major {
        println!("layout major:   FM{} (re-saved by a newer FM)", v.layout_major());
    }
    println!("db version:     {:?} (tag {:?}, game_db format {:?})", v.db_version, v.db_tag, v.game_db_format);
    match fm_save_parser::layout::layout_for(v.layout_major()) {
        Some(l) => println!("layout:         {} ({:?})", l.name, l.confidence),
        None => println!("layout:         none registered; WARNING: FM{}: {}", v.major, fm_save_parser::UNVERIFIED_WARNING),
    }
    println!("entries:        {} ({} bytes uncompressed)", c.entries.len(), total);
    Ok(())
}

fn cmd_ls(path: &str) -> Result<()> {
    let c = FmContainer::open(Path::new(path))?;
    println!("{:>12} {:>12} {:>12}  path", "offset", "compressed", "size");
    for e in &c.entries {
        println!("{:>12} {:>12} {:>12}  {}", e.offset, e.compressed_size, e.uncompressed_size, e.path);
    }
    Ok(())
}

fn cmd_extract(path: &str, entry: &str, out: &str) -> Result<()> {
    let c = FmContainer::open(Path::new(path))?;
    let data = c.read(entry)?;
    std::fs::write(out, &data)?;
    println!("wrote {} bytes to {out}", data.len());
    Ok(())
}

fn cmd_players(path: &str, opts: &[String]) -> Result<()> {
    let mut top = 30usize;
    let mut min_ca = 0u16;
    let mut json: Option<String> = None;
    let mut nation_file: Option<String> = None;
    let mut force_probe = false;
    let mut it = opts.iter();
    while let Some(o) = it.next() {
        let mut val = || it.next().cloned().ok_or_else(|| FmError::InvalidFile(format!("{o} needs a value")));
        match o.as_str() {
            "--top" => top = val()?.parse().map_err(|_| FmError::InvalidFile("--top expects a number".into()))?,
            "--min-ca" => min_ca = val()?.parse().map_err(|_| FmError::InvalidFile("--min-ca expects a number".into()))?,
            "--json" => json = Some(val()?),
            "--nation-names" => nation_file = Some(val()?),
            "--force-probe" => force_probe = true,
            other => return Err(FmError::InvalidFile(format!("unknown option {other}"))),
        }
    }
    let t = Instant::now();
    let c = FmContainer::open(Path::new(path))?;
    let opts = fm_save_parser::Options { nation_names: nation_file.as_deref().map(std::fs::read_to_string).transpose()?, force_probe };
    let mut data = fm_save_parser::analyze(&c, &opts)?;
    data.players.retain(|p| p.current_ability >= min_ca);
    for w in &data.warnings {
        eprintln!("WARNING: {w}");
    }
    let named = data.players.iter().filter(|p| p.name.is_some()).count();
    eprintln!(
        "FM{} ({}) layout {} [{:?}]: {} players, {} named, {:.1}s",
        data.version.major,
        data.version.game_version.as_deref().unwrap_or("?"),
        data.layout.name,
        data.layout.confidence,
        data.players.len(),
        named,
        t.elapsed().as_secs_f32()
    );
    if let Some(out) = json {
        let f = std::io::BufWriter::new(std::fs::File::create(&out)?);
        serde_json::to_writer(f, &data)?;
        eprintln!("wrote {out}");
        return Ok(());
    }
    let mut list = data.players;
    list.sort_by(|a, b| b.current_ability.cmp(&a.current_ability));
    println!("{:<28} {:>4} {:>4} {:>3} {:<10} {:>4} {:<24}", "name", "CA", "PA", "pos", "born", "nat", "club");
    for p in list.iter().take(top) {
        let club = match &p.club {
            Some(c) if c.team_slot == 0 => c.short_name.clone().unwrap_or_default(),
            Some(c) => format!("{} (team {})", c.short_name.as_deref().unwrap_or(""), c.team_slot + 1),
            None => p.club_id.map(|id| format!("#{id}")).unwrap_or_else(|| "-".into()),
        };
        println!(
            "{:<28} {:>4} {:>4} {:>3} {:<10} {:>4} {:<24}",
            p.name.as_deref().unwrap_or("?"),
            p.current_ability,
            p.potential_ability,
            p.best_position,
            p.date_of_birth.as_deref().unwrap_or("?"),
            p.nation_index.map(|n| n.to_string()).unwrap_or_default(),
            club
        );
    }
    Ok(())
}
