"""OpenAI-compatible translation adapter using only the Python standard library."""

from __future__ import annotations

import ipaddress
import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Protocol, cast

from workers.common.protocol import JsonObject, JsonValue, ProtocolError
from workers.translation.contract import TemporaryProviderError


class HttpTransport(Protocol):
    def post_json(self, url: str, headers: dict[str, str], payload: JsonObject) -> JsonObject: ...


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args: object, **kwargs: object) -> None:
        return None


class StdlibHttpsTransport:
    def post_json(self, url: str, headers: dict[str, str], payload: JsonObject) -> JsonObject:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        # The adapter constructor accepts only validated HTTPS endpoints.
        request = urllib.request.Request(  # noqa: S310
            url, data=body, headers=headers, method="POST"
        )
        opener = urllib.request.build_opener(_NoRedirect())
        try:
            with opener.open(request, timeout=60) as response:
                content = response.read(4 * 1024 * 1024 + 1)
        except (urllib.error.URLError, TimeoutError) as error:
            raise TemporaryProviderError from error
        if len(content) > 4 * 1024 * 1024:
            raise ProtocolError("PROVIDER_RESPONSE_TOO_LARGE", "Phản hồi dịch vượt giới hạn.")
        try:
            value = cast(JsonValue, json.loads(content))
        except json.JSONDecodeError as error:
            raise ProtocolError(
                "INVALID_PROVIDER_RESPONSE", "Nhà cung cấp trả dữ liệu không hợp lệ."
            ) from error
        if not isinstance(value, dict):
            raise ProtocolError(
                "INVALID_PROVIDER_RESPONSE", "Nhà cung cấp trả dữ liệu không hợp lệ."
            )
        return value


class OpenAiCompatibleAdapter:
    provider_id = "openai-compatible"
    sends_data_off_device = True

    def __init__(
        self,
        endpoint: str,
        model: str,
        api_key: str,
        transport: HttpTransport | None = None,
    ) -> None:
        self.endpoint = _validate_endpoint(endpoint)
        if not model or len(model) > 200 or not api_key:
            raise ProtocolError(
                "INVALID_PROVIDER_CONFIG", "Cấu hình nhà cung cấp dịch không hợp lệ."
            )
        self.model = model
        self._api_key = api_key
        self._transport = transport or StdlibHttpsTransport()

    def translate(self, system_prompt: str, user_prompt: str) -> str:
        response = self._transport.post_json(
            self.endpoint,
            {
                "Authorization": f"Bearer {self._api_key}",
                "Content-Type": "application/json",
            },
            {
                "model": self.model,
                "temperature": 0,
                "response_format": {"type": "json_object"},
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": user_prompt},
                ],
            },
        )
        try:
            choices = cast(list[JsonObject], response["choices"])
            message = cast(JsonObject, choices[0]["message"])
            content = message["content"]
        except (KeyError, IndexError, TypeError) as error:
            raise ProtocolError(
                "INVALID_PROVIDER_RESPONSE", "Nhà cung cấp trả dữ liệu không hợp lệ."
            ) from error
        if not isinstance(content, str):
            raise ProtocolError(
                "INVALID_PROVIDER_RESPONSE", "Nhà cung cấp trả dữ liệu không hợp lệ."
            )
        return content


def _validate_endpoint(value: str) -> str:
    parsed = urllib.parse.urlsplit(value)
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.fragment
        or parsed.port not in {None, 443}
    ):
        raise ProtocolError("INVALID_PROVIDER_ENDPOINT", "Endpoint dịch không an toàn.")
    try:
        address = ipaddress.ip_address(parsed.hostname)
    except ValueError:
        address = None
    if address is not None and not address.is_global:
        raise ProtocolError("INVALID_PROVIDER_ENDPOINT", "Endpoint dịch không an toàn.")
    return value
