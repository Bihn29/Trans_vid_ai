import { t } from "../lib/i18n";

interface WorkflowStepsProps {
  activeStep: number;
  onSelect: (step: number) => void;
}

export function WorkflowSteps({ activeStep, onSelect }: WorkflowStepsProps) {
  const steps = [t("sourceVideo"), t("transcript"), t("translation"), t("voice"), t("previewExport")];

  return (
    <ol className="workflow-steps" aria-label="Các bước dự án">
      {steps.map((step, index) => (
        <li key={step}>
          <button
            aria-current={index === activeStep ? "step" : undefined}
            className={index === activeStep ? "workflow-step workflow-step--active" : "workflow-step"}
            onClick={() => onSelect(index)}
            type="button"
          >
            <span>{index + 1}</span>
            <strong>{step}</strong>
          </button>
        </li>
      ))}
    </ol>
  );
}
