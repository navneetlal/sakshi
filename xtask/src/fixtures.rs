//! Regenerates `fixtures/` from pinned, openly licensed sources. The
//! sources are downloaded once into `target/fixture-sources/` and checked
//! against SHA-256; `fixtures/README.md` documents each clip.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, bail};

use crate::util::{download, run, verify_sha256, workspace_root};

struct Source {
    file: &'static str,
    url: &'static str,
    sha256: &'static str,
}

const AMI_BASE: &str = "https://groups.inf.ed.ac.uk/ami/AMICorpusMirror/amicorpus/EN2002d/audio";

/// AMI meeting EN2002d individual headset channels (CC BY 4.0).
/// Channel to speaker, from mean loudness during each speaker's reference
/// turns: 0 = FEO070, 1 = FEO072, 2 = MEE071, 3 = MEE073.
const AMI_HEADSETS: [Source; 4] = [
    Source {
        file: "ami/EN2002d.Headset-0.wav",
        url: "EN2002d.Headset-0.wav",
        sha256: "88cf01860709899d62d9466d145838d7b64d8b959ab82c041ae73c9c6ad06b5d",
    },
    Source {
        file: "ami/EN2002d.Headset-1.wav",
        url: "EN2002d.Headset-1.wav",
        sha256: "28486ee15ec50a876274ccf014992338cb68df9c5e56de7a6a670a7bb8e3dccd",
    },
    Source {
        file: "ami/EN2002d.Headset-2.wav",
        url: "EN2002d.Headset-2.wav",
        sha256: "a0709a1431afe4ad336d2a82be1d73b491f96477d306863cc756a16693213902",
    },
    Source {
        file: "ami/EN2002d.Headset-3.wav",
        url: "EN2002d.Headset-3.wav",
        sha256: "5e38b0f277f40107573f4492d44e2ec0bd1295e32e2cbf95ab4f3d137166f575",
    },
];

/// Reference speaker turns and words for the same excerpt, from
/// NeMo-Speech.cpp v0.2.0 `test_files/diar/`.
const AMI_REFERENCE: [Source; 2] = [
    Source {
        file: "ami/ami_en2002d_2132.rttm",
        url: "https://raw.githubusercontent.com/NVIDIA/NeMo-Speech.cpp/v0.2.0/test_files/diar/ami_en2002d_2132.rttm",
        sha256: "40751c9fc0bb934f741789ce3fafccd2c1f9d7c112585b8b40ac177a9de13b2d",
    },
    Source {
        file: "ami/ami_en2002d_2132.json",
        url: "https://raw.githubusercontent.com/NVIDIA/NeMo-Speech.cpp/v0.2.0/test_files/diar/ami_en2002d_2132.json",
        sha256: "8a77aab95e65e301f3cb23cfd0a0641b3295a9da5686ff30f2ab12d0f282e229",
    },
];

/// Excerpt of EN2002d used by the NeMo-Speech.cpp diarization fixture.
const AMI_START_S: &str = "2132.5";
const AMI_LENGTH_S: &str = "60";
/// FEO070 plays "You"; the other three are the remote participants.
const YOU_HEADSET: usize = 0;
/// Remote audio reaching the mic from the speakers, about -10.5 dB.
const BLEED_GAIN: &str = "0.3";

const FLEURS_BASE: &str = "https://huggingface.co/datasets/google/fleurs/resolve/70bb2e84b976b7e960aa89f1c648e09c59f894dd/data/hi_in";

/// Google FLEURS Hindi dev split (CC BY 4.0).
const FLEURS: [Source; 2] = [
    Source {
        file: "fleurs-hi/dev.tsv",
        url: "dev.tsv",
        sha256: "cea87c57a37a0d38ed0afce30e68a35ad7b3945414648430fee09a54ca7b72ba",
    },
    Source {
        file: "fleurs-hi/dev.tar.gz",
        url: "audio/dev.tar.gz",
        sha256: "9adbca6d6fc70e40c121910941bcd7c8906eee60b402b6d21b4bd160e20030c7",
    },
];

/// One female and one male utterance, so the clip has two different
/// speakers (FLEURS has no speaker ids).
const HINDI_TURNS: [&str; 2] = ["5958463932227431376", "14584887621258891555"];

pub fn generate() -> anyhow::Result<()> {
    let root = workspace_root();
    let sources = root.join("target").join("fixture-sources");
    let fixtures = root.join("fixtures");

    for s in &AMI_HEADSETS {
        fetch(&sources, s, Some(AMI_BASE))?;
    }
    for s in AMI_REFERENCE.iter() {
        fetch(&sources, s, None)?;
    }
    for s in &FLEURS {
        fetch(&sources, s, Some(FLEURS_BASE))?;
    }

    ami(&sources, &fixtures.join("ami-en2002d"))?;
    hindi(&sources, &fixtures.join("fleurs-hi"))?;
    println!("fixtures written to {}", fixtures.display());
    Ok(())
}

fn fetch(sources: &Path, s: &Source, base: Option<&str>) -> anyhow::Result<()> {
    let path = sources.join(s.file);
    if !path.exists() || verify_sha256(&path, s.sha256).is_err() {
        let url = match base {
            Some(base) => format!("{base}/{}", s.url),
            None => s.url.to_owned(),
        };
        download(&url, &path)?;
    }
    verify_sha256(&path, s.sha256)
}

fn ffmpeg() -> Command {
    let bin = std::env::var_os("SAKSHI_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
    let mut cmd = Command::new(bin);
    cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]);
    cmd
}

/// Output flags that keep encoder versions and metadata out of the files.
const BITEXACT: [&str; 4] = ["-fflags", "+bitexact", "-map_metadata", "-1"];
const WAV_16K: [&str; 8] = [
    "-ac",
    "1",
    "-ar",
    "16000",
    "-c:a",
    "pcm_s16le",
    "-flags:a",
    "+bitexact",
];

fn ami(sources: &Path, out: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(out)?;
    let excerpt = |cmd: &mut Command, headset: usize| {
        cmd.args(["-ss", AMI_START_S, "-t", AMI_LENGTH_S, "-i"])
            .arg(sources.join(AMI_HEADSETS[headset].file));
    };
    let remote: Vec<usize> = (0..4).filter(|&h| h != YOU_HEADSET).collect();

    // system.wav: the three remote headsets mixed.
    let mut cmd = ffmpeg();
    for &h in &remote {
        excerpt(&mut cmd, h);
    }
    run(cmd
        .args([
            "-filter_complex",
            "[0][1][2]amix=inputs=3:normalize=0[a]",
            "-map",
            "[a]",
        ])
        .args(WAV_16K)
        .args(BITEXACT)
        .arg(out.join("system.wav")))
    .context("mixing system.wav")?;

    // mic.wav: the user's headset plus attenuated remote audio as bleed.
    let mut cmd = ffmpeg();
    excerpt(&mut cmd, YOU_HEADSET);
    for &h in &remote {
        excerpt(&mut cmd, h);
    }
    let filter = format!(
        "[1][2][3]amix=inputs=3:normalize=0,volume={BLEED_GAIN}[bleed];[0][bleed]amix=inputs=2:normalize=0[a]"
    );
    run(cmd
        .args(["-filter_complex", &filter, "-map", "[a]"])
        .args(WAV_16K)
        .args(BITEXACT)
        .arg(out.join("mic.wav")))
    .context("mixing mic.wav")?;

    // recording.mp4: three static 20 s test patterns, so key frames are
    // expected at 0, 20, and 40 s and nowhere else.
    run(ffmpeg()
        .args([
            "-f",
            "lavfi",
            "-i",
            "smptebars=size=640x360:rate=5:duration=20",
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            "colorchart=rate=5:duration=20,scale=640x360,setsar=1",
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            "rgbtestsrc=size=640x360:rate=5:duration=20",
        ])
        .args([
            "-filter_complex",
            "[0:v][1:v][2:v]concat=n=3:v=1[v]",
            "-map",
            "[v]",
        ])
        .args([
            "-c:v",
            "mpeg4",
            "-q:v",
            "6",
            "-flags:v",
            "+bitexact",
            "-t",
            "60",
        ])
        .args(BITEXACT)
        .arg(out.join("recording.mp4")))
    .context("generating recording.mp4")?;

    for (src, name) in AMI_REFERENCE
        .iter()
        .zip(["reference.rttm", "reference.json"])
    {
        std::fs::copy(sources.join(src.file), out.join(name))?;
    }
    Ok(())
}

fn hindi(sources: &Path, out: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(out)?;
    let audio_dir = sources.join("fleurs-hi");
    let members: Vec<String> = HINDI_TURNS
        .iter()
        .map(|id| format!("dev/{id}.wav"))
        .collect();
    run(Command::new("tar")
        .arg("-xzf")
        .arg(sources.join(FLEURS[1].file))
        .arg("-C")
        .arg(&audio_dir)
        .args(&members))
    .context("extracting FLEURS utterances")?;

    // Female turn, one second of silence, male turn.
    run(ffmpeg()
        .arg("-i")
        .arg(audio_dir.join(&members[0]))
        .args(["-f", "lavfi", "-t", "1", "-i", "anullsrc=r=16000:cl=mono"])
        .arg("-i")
        .arg(audio_dir.join(&members[1]))
        .args([
            "-filter_complex",
            "[0][1][2]concat=n=3:v=0:a=1[a]",
            "-map",
            "[a]",
        ])
        .args(WAV_16K)
        .args(BITEXACT)
        .arg(out.join("system.wav")))
    .context("building the Hindi clip")?;

    let tsv = std::fs::read_to_string(sources.join(FLEURS[0].file))?;
    let mut reference = String::from("turn\tfleurs_id\tgender\ttranscription\n");
    for (turn, id) in HINDI_TURNS.iter().enumerate() {
        let row = tsv
            .lines()
            .map(|l| l.split('\t').collect::<Vec<_>>())
            .find(|cols| cols.get(1) == Some(&format!("{id}.wav").as_str()))
            .with_context(|| format!("{id} not in dev.tsv"))?;
        let (raw_text, gender) = match row.as_slice() {
            [_, _, raw, _, _, _, gender, ..] => (raw, gender),
            _ => bail!("unexpected dev.tsv row for {id}"),
        };
        reference.push_str(&format!("{}\t{id}\t{gender}\t{raw_text}\n", turn + 1));
    }
    std::fs::write(out.join("reference.tsv"), reference)?;
    Ok(())
}
