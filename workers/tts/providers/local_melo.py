from __future__ import annotations

import contextlib
import io
import os
import tempfile
from pathlib import Path
from typing import Any

from workers.common.protocol import ProtocolError


class LocalMeloTtsAdapter:
    provider_id = "local-melo"
    sends_data_off_device = False

    def __init__(self, model_root: str, scratch_directory: Path) -> None:
        root = Path(model_root)
        try:
            root = root.resolve(strict=True)
            config = (root / "config.json").resolve(strict=True)
            checkpoint = (root / "G_463000.pth").resolve(strict=True)
            dictionary = (root / "vie-n.tsv").resolve(strict=True)
            scratch = scratch_directory.resolve(strict=True)
        except OSError as error:
            raise ProtocolError(
                "LOCAL_MODEL_NOT_AVAILABLE",
                "Mô hình giọng Việt cục bộ chưa được cài đặt đầy đủ.",
            ) from error
        if (
            root.is_symlink()
            or not root.is_dir()
            or not all(
                path.is_file() and path.is_relative_to(root)
                for path in (config, checkpoint, dictionary)
            )
            or scratch.is_symlink()
            or not scratch.is_dir()
        ):
            raise ProtocolError("UNSAFE_PATH", "Đường dẫn mô hình giọng Việt không hợp lệ.")

        original = Path.cwd()
        try:
            os.chdir(root)
            with contextlib.redirect_stdout(io.StringIO()):
                from melo.api import TTS  # type: ignore[import-not-found]

                self._model: Any = TTS(
                    language="VI",
                    device="cpu",
                    config_path=str(config),
                    ckpt_path=str(checkpoint),
                )
        except Exception as error:
            raise ProtocolError(
                "LOCAL_TTS_INITIALIZATION_FAILED",
                "Không thể khởi tạo mô hình giọng Việt cục bộ.",
            ) from error
        finally:
            os.chdir(original)
        self._speaker_id = int(self._model.hps.data.spk2id["VI-default"])
        self._scratch = scratch

    def synthesize(self, text: str, voice_id: str, speed: float) -> bytes:
        if (
            voice_id != "vi-default"
            or not text.strip()
            or len(text) > 4096
            or not 0.5 <= speed <= 2.0
        ):
            raise ProtocolError("INVALID_TTS_REQUEST", "Yêu cầu giọng Việt không hợp lệ.")
        descriptor, temporary = tempfile.mkstemp(
            prefix=".melotts-", suffix=".wav", dir=self._scratch
        )
        os.close(descriptor)
        path = Path(temporary)
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                self._model.tts_to_file(
                    text.strip(), self._speaker_id, str(path), speed=speed, quiet=True
                )
            return path.read_bytes()
        except Exception as error:
            raise ProtocolError(
                "LOCAL_TTS_FAILED", "Không thể tạo giọng đọc cho một đoạn phụ đề."
            ) from error
        finally:
            path.unlink(missing_ok=True)
