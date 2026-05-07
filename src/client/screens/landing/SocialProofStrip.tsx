import { PROOF_LABELS } from '../../../data/landing-niches';

const TRIPLED = [...PROOF_LABELS, ...PROOF_LABELS, ...PROOF_LABELS];

export function SocialProofStrip() {
  return (
    <section className="lp-proof">
      <p className="lp-proof__label">Trusted by 10,000+ creators</p>
      <div className="lp-marquee">
        <div className="lp-marquee__track">
          {TRIPLED.map((label, i) => (
            <span key={i} className="lp-marquee__chip">{label}</span>
          ))}
        </div>
      </div>
    </section>
  );
}
