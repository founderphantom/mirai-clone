import { motion } from 'motion/react';

interface CtaBannerProps {
  onGetStarted: () => void;
}

export function CtaBanner({ onGetStarted }: CtaBannerProps) {
  return (
    <section className="lp-cta-banner">
      <motion.div
        initial={{ opacity: 0, y: 32 }}
        whileInView={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.5 }}
        viewport={{ once: true }}
      >
        <h2 className="lp-cta-banner__h2">Your next post is one swipe away</h2>
        <p className="lp-cta-banner__sub">Clone your vibe for free. No credit card required.</p>
        <button className="lp-btn-primary lp-cta-banner__btn" onClick={onGetStarted}>
          Create your free clone →
        </button>
      </motion.div>
    </section>
  );
}
