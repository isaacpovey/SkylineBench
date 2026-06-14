export const BrandMark = ({ className }: { className?: string }) => (
  <svg className={className} viewBox="0 0 28 28" fill="none" aria-hidden="true">
    <rect x="2" y="14" width="4" height="10" rx="1" fill="currentColor" opacity="0.55" />
    <rect x="8" y="9" width="4" height="15" rx="1" fill="currentColor" opacity="0.8" />
    <rect x="14" y="4" width="4" height="20" rx="1" fill="var(--skl-blue-bright)" />
    <rect x="20" y="11" width="4" height="13" rx="1" fill="currentColor" opacity="0.65" />
    <line x1="1" y1="25.5" x2="27" y2="25.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" opacity="0.4" />
  </svg>
);
