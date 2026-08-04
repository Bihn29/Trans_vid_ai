from __future__ import annotations

import json

import pytest

from workers.common.protocol import JsonObject, ProtocolError
from workers.translation.providers.local import LocalTranslationAdapter
from workers.translation.providers.openai_compatible import OpenAiCompatibleAdapter


class _HttpTransport:
    def __init__(self) -> None:
        self.headers: dict[str, str] = {}
        self.payload: JsonObject = {}

    def post_json(self, _url: str, headers: dict[str, str], payload: JsonObject) -> JsonObject:
        self.headers = headers
        self.payload = payload
        return {"choices": [{"message": {"content": json.dumps({"ok": True})}}]}


def test_openai_compatible_adapter_formats_request_without_logging_secret() -> None:
    transport = _HttpTransport()
    adapter = OpenAiCompatibleAdapter(
        "https://api.example.com/v1/chat/completions", "test-model", "secret-token", transport
    )
    assert adapter.translate("system", "user") == '{"ok": true}'
    assert transport.headers["Authorization"] == "Bearer secret-token"
    assert transport.payload["model"] == "test-model"
    assert adapter.sends_data_off_device is True


@pytest.mark.parametrize(
    "endpoint",
    [
        "http://api.example.com/v1/chat/completions",
        "https://127.0.0.1/v1/chat/completions",
        "https://user:pass@api.example.com/v1/chat/completions",
        "https://api.example.com:8443/v1/chat/completions",
    ],
)
def test_openai_compatible_adapter_rejects_unsafe_endpoints(endpoint: str) -> None:
    with pytest.raises(ProtocolError):
        OpenAiCompatibleAdapter(endpoint, "model", "secret", _HttpTransport())


class _LocalTransport:
    def translate_locally(self, _system: str, _user: str) -> str:
        return "local-result"


def test_local_adapter_contract_never_claims_cloud_transfer() -> None:
    adapter = LocalTranslationAdapter(_LocalTransport())
    assert adapter.translate("system", "user") == "local-result"
    assert adapter.sends_data_off_device is False
