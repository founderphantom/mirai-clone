import { useState } from 'react';
import { Check, Minus } from 'lucide-react';

interface PricingSectionProps {
  onGetStarted: () => void;
}

const PLANS = [
  {
    tier: 'Free',
    monthlyPrice: '$0',
    yearlyPrice: '$0',
    yearlyBilled: null,
    credits: '10 signup credits',
    blitz: '5 Blitz cards/day',
    watermark: true,
    quality: '1080p',
    characters: '1 Starter Character',
    videoBlitz: false,
    trial: false,
    cta: 'Get started',
    popular: false,
  },
  {
    tier: 'Pro',
    monthlyPrice: '$14.99',
    yearlyPrice: '$11.99',
    yearlyBilled: '$143.88/yr',
    credits: '300 credits/mo',
    blitz: '25 Blitz cards/day',
    watermark: false,
    quality: 'HD',
    characters: 'All 10 Characters',
    videoBlitz: false,
    trial: true,
    cta: 'Start free trial',
    popular: true,
  },
  {
    tier: 'Studio',
    monthlyPrice: '$39.99',
    yearlyPrice: '$31.99',
    yearlyBilled: '$383.88/yr',
    credits: '1,500 credits/mo',
    blitz: '100 Blitz cards/day',
    watermark: false,
    quality: 'HD',
    characters: 'All 10 + remix',
    videoBlitz: true,
    trial: true,
    cta: 'Start free trial',
    popular: false,
  },
];

function FeatureRow({ included, label }: { included: boolean; label: string }) {
  return (
    <li className="lp-pricing-card__feature">
      {included ? <Check size={15} className="lp-pricing-check" /> : <Minus size={15} className="lp-pricing-minus" />}
      <span>{label}</span>
    </li>
  );
}

export function PricingSection({ onGetStarted }: PricingSectionProps) {
  const [isYearly, setIsYearly] = useState(false);

  return (
    <section className="lp-section" id="pricing">
      <div className="lp-container">
        <h2 className="lp-h2">Simple, creator-first pricing</h2>
        <div className="lp-pricing-toggle">
          <span className={!isYearly ? 'lp-toggle-label--active' : 'lp-toggle-label'}>Monthly</span>
          <button
            className="lp-toggle-switch"
            onClick={() => setIsYearly((v) => !v)}
            aria-label="Toggle yearly billing"
            aria-pressed={isYearly}
          >
            <span className={`lp-toggle-switch__thumb${isYearly ? ' lp-toggle-switch__thumb--on' : ''}`} />
          </button>
          <span className={isYearly ? 'lp-toggle-label--active' : 'lp-toggle-label'}>Yearly <span className="lp-toggle-save">Save 20%</span></span>
        </div>
        <div className="lp-pricing-cards">
          {PLANS.map((plan) => {
            const price = isYearly ? plan.yearlyPrice : plan.monthlyPrice;
            const billedLine = isYearly && plan.yearlyBilled ? plan.yearlyBilled : ' ';
            return (
              <div
                key={plan.tier}
                className={`lp-pricing-card${plan.popular ? ' lp-pricing-card--popular' : ''}`}
              >
                {plan.popular && (
                  <div className="lp-pricing-card__popular-badge">Most popular</div>
                )}
                <p className="lp-pricing-card__tier">{plan.tier}</p>
                <div className="lp-pricing-card__price">
                  <span className="lp-pricing-card__amount">{price}</span>
                  <span className="lp-pricing-card__period">/mo</span>
                </div>
                <p className="lp-pricing-card__billed">{billedLine}</p>
                <button
                  className={plan.popular ? 'lp-btn-primary lp-pricing-card__cta' : 'lp-btn-ghost lp-pricing-card__cta'}
                  onClick={onGetStarted}
                >
                  {plan.cta}
                </button>
                <ul className="lp-pricing-card__features">
                  <FeatureRow included={true} label={plan.credits} />
                  <FeatureRow included={true} label={plan.blitz} />
                  <FeatureRow included={!plan.watermark} label="No watermark" />
                  <FeatureRow included={true} label={plan.quality} />
                  <FeatureRow included={true} label={plan.characters} />
                  <FeatureRow included={plan.videoBlitz} label="Video Blitz drops" />
                </ul>
              </div>
            );
          })}
        </div>
      </div>
    </section>
  );
}
