#!/usr/bin/env python3
"""Speech-to-text helper for Zolly's Telegram voice notes.

Usage: stt.py <audio-file>

Downloads nothing. Decodes the input with ffmpeg to 16 kHz mono s16le PCM,
feeds it to faster-whisper (base model, CPU int8), prints the transcript to
stdout. Model cache lives at ~/.config/zolly/stt.

Exit codes:
  0  transcript printed (may be empty for silence)
  1  decode or model error (message on stderr)
"""
import os
import subprocess
import sys
import tempfile

_SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
_VENV_PYTHON = os.path.join(_SCRIPT_DIR, ".venv-stt", "bin", "python3")


def _ensure_venv_interpreter() -> None:
    """Re-exec under the pinned venv interpreter so a different interpreter
    can never be silently picked up. If the venv is absent we fall through
    and the dependency check below reports the missing venv cleanly."""
    if os.path.abspath(sys.executable) == os.path.abspath(_VENV_PYTHON):
        return
    if os.path.isfile(_VENV_PYTHON):
        os.execv(_VENV_PYTHON, [_VENV_PYTHON, os.path.abspath(__file__)] + sys.argv[1:])


def _load_deps() -> None:
    """Import the heavy deps. On failure print one clean line to stderr and
    exit 1 — never a traceback."""
    try:
        import numpy as np  # noqa: F401
        from faster_whisper import WhisperModel  # noqa: F401
    except ImportError as e:
        missing = getattr(e, "name", str(e))
        print(
            f"stt: missing dependency '{missing}' in the stt venv; "
            f"run with {_VENV_PYTHON}",
            file=sys.stderr,
        )
        sys.exit(1)


STT_CACHE = os.path.expanduser("~/.config/zolly/stt")


def decode_audio(path: str):
    """Decode any audio file to 16 kHz mono float32 via ffmpeg."""
    import numpy as np

    result = subprocess.run(
        [
            "ffmpeg", "-i", path,
            "-ar", "16000", "-ac", "1",
            "-f", "s16le", "-acodec", "pcm_s16le",
            "-",
        ],
        capture_output=True,
    )
    if result.returncode != 0:
        raise RuntimeError(f"ffmpeg decode failed: {result.stderr.decode()[:200]}")
    raw = np.frombuffer(result.stdout, dtype=np.int16)
    return raw.astype(np.float32) / 32768.0


def transcribe(audio) -> str:
    from faster_whisper import WhisperModel

    model = WhisperModel(
        "base", device="cpu", compute_type="int8", download_root=STT_CACHE
    )
    segments, _info = model.transcribe(audio, language="en", vad_filter=True)
    return "".join(seg.text for seg in segments).strip()


def main() -> None:
    _ensure_venv_interpreter()
    _load_deps()
    if len(sys.argv) != 2:
        print("usage: stt.py <audio-file>", file=sys.stderr)
        sys.exit(1)
    path = sys.argv[1]
    if not os.path.isfile(path):
        print(f"not found: {path}", file=sys.stderr)
        sys.exit(1)
    try:
        audio = decode_audio(path)
        if len(audio) < 1600:  # < 0.1 s
            print("")
            return
        text = transcribe(audio)
        print(text)
    except Exception as e:
        print(f"stt error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
