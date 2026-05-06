import { SwipeDeckHero } from './SwipeDeckHero';

interface HeroSectionProps {
  onGetStarted: () => void;
}

export function HeroSection({ onGetStarted }: HeroSectionProps) {
  const scrollToHowItWorks = (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    document.querySelector('#how-it-works')?.scrollIntoView({ behavior: 'smooth' });
  };

  return (
    <section className="lp-hero">
      <div className="lp-container">
        <div className="lp-hero__grid">
          <div className="lp-hero__left">
            <span className="lp-hero__badge">✦ Now in early access</span>
            <h1 className="lp-h1">
              The fastest way to put{' '}
              <span className="lp-accent-underline">yourself</span>{' '}
              in every trend
            </h1>
            <p className="lp-hero__sub">
              Daily AI drops of you in Y2K Cafe, Tokyo Neon, Cottagecore and 50+ aesthetics. Swipe to save. No prompting.
            </p>
            <div className="lp-hero__cta-row">
              <button className="lp-btn-primary" onClick={onGetStarted}>Start for free</button>
              <button className="lp-btn-ghost" onClick={scrollToHowItWorks}>See how it works</button>
            </div>
            <p className="lp-hero__social-proof">✦ 10,000+ creators · No credit card required</p>
          </div>
          <div className="lp-hero__right">
            <SwipeDeckHero />
          </div>
        </div>
      </div>
    </section>
  );
}
