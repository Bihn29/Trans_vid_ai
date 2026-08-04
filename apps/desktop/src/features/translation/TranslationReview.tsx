import { useMemo, useState } from "react";

import type {
  TranslationProviderDisclosure,
  TranslationReviewRow,
} from "../../types/translation";

interface TranslationReviewProps {
  providers: TranslationProviderDisclosure[];
  rows: TranslationReviewRow[];
  onSave?: (segmentId: string, translatedText: string) => void;
  onApprove?: () => void;
}

export function TranslationReview({ providers, rows, onSave, onApprove }: TranslationReviewProps) {
  const [providerId, setProviderId] = useState(providers[0]?.providerId ?? "");
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const provider = useMemo(
    () => providers.find((candidate) => candidate.providerId === providerId),
    [providerId, providers],
  );

  return (
    <section className="translation-review" aria-labelledby="translation-review-heading">
      <div className="section-heading">
        <div>
          <h2 id="translation-review-heading">Rà soát bản dịch</h2>
        </div>
        <label className="provider-picker">
          Nhà cung cấp
          <select
            aria-label="Nhà cung cấp dịch"
            onChange={(event) => setProviderId(event.target.value)}
            value={providerId}
          >
            {providers.map((candidate) => (
              <option key={candidate.providerId} value={candidate.providerId}>
                {candidate.displayName}
              </option>
            ))}
          </select>
        </label>
      </div>

      {provider?.sendsDataOffDevice ? (
        <div className="cloud-disclosure" role="alert">
          Nhà cung cấp này gửi nội dung transcript ra ngoài thiết bị. Bạn phải xác nhận trước khi
          bắt đầu dịch.
        </div>
      ) : (
        <p className="local-disclosure">Nhà cung cấp đã chọn xử lý nội dung trên thiết bị.</p>
      )}

      {rows.length === 0 ? (
        <p className="translation-empty">Bản dịch sẽ xuất hiện ở đây sau khi transcript được duyệt.</p>
      ) : (
        <div className="translation-rows">
          {rows.map((row) => {
            const value = drafts[row.id] ?? row.translatedText;
            const missingLockedName = row.lockedNames.some(
              (name) => row.sourceText.includes(name) && !value.includes(name),
            );
            return (
              <article className="translation-row" key={row.id}>
                <p lang="zh">{row.sourceText}</p>
                <textarea
                  aria-label={`Bản dịch ${row.id}`}
                  onChange={(event) =>
                    setDrafts((current) => ({ ...current, [row.id]: event.target.value }))
                  }
                  value={value}
                />
                {missingLockedName ? (
                  <span className="translation-error">Tên riêng bị khóa phải được giữ nguyên.</span>
                ) : null}
                <button
                  disabled={!value.trim() || missingLockedName}
                  onClick={() => onSave?.(row.id, value)}
                  type="button"
                >
                  Lưu đoạn dịch
                </button>
              </article>
            );
          })}
        </div>
      )}

      <button disabled={rows.length === 0} onClick={onApprove} type="button">
        Duyệt bản dịch
      </button>
    </section>
  );
}
