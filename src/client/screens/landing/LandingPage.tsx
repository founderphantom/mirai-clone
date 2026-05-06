import './landing.css';
import { LandingNav } from './LandingNav';
import { HeroSection } from './HeroSection';
import { SocialProofStrip } from './SocialProofStrip';
import { HowItWorksSection } from './HowItWorksSection';
import { GalleryStrip } from './GalleryStrip';
import { FeatureGrid } from './FeatureGrid';
import { TestimonialsSection } from './TestimonialsSection';
import { PricingSection } from './PricingSection';
import { CtaBanner } from './CtaBanner';
import { LandingFooter } from './LandingFooter';

interface LandingPageProps {
  onGetStarted: () => void;
}

export function LandingPage({ onGetStarted }: LandingPageProps) {
  return (
    <div className="lp-root">
      <LandingNav onGetStarted={onGetStarted} />
      <HeroSection onGetStarted={onGetStarted} />
      <SocialProofStrip />
      <HowItWorksSection />
      <GalleryStrip />
      <FeatureGrid />
      <TestimonialsSection />
      <PricingSection onGetStarted={onGetStarted} />
      <CtaBanner onGetStarted={onGetStarted} />
      <LandingFooter />
    </div>
  );
}
