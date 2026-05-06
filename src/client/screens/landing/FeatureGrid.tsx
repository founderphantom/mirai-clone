import { motion } from 'motion/react';
import { Zap, Sparkles, TrendingUp, Share2, MessageCircle, Video } from 'lucide-react';

const FEATURES = [
  { Icon: Zap, title: 'Blitz Mode', desc: 'Daily AI drops generated overnight. Wake up, swipe, post.' },
  { Icon: Sparkles, title: 'Soul Clone', desc: 'AI trained on your face — not a filter. Every output is uniquely you.' },
  { Icon: TrendingUp, title: '50+ Aesthetics', desc: 'Y2K, Neon Tokyo, Cottagecore and more, updated weekly with trends.' },
  { Icon: Share2, title: '1-tap Export', desc: 'Reel, Story, and Post sizes auto-generated. Ready to upload instantly.' },
  { Icon: MessageCircle, title: 'Zero Prompting', desc: 'No prompt writing. Pick aesthetics once, get infinite variations.' },
  { Icon: Video, title: 'Video Drops', desc: 'Short clips for Studio users. The future of creator content.' },
];

export function FeatureGrid() {
  return (
    <section className="lp-section lp-section--alt">
      <div className="lp-container">
        <h2 className="lp-h2">Everything a creator needs</h2>
        <div className="lp-features">
          {FEATURES.map(({ Icon, title, desc }) => (
            <motion.div
              key={title}
              className="lp-feature-card"
              whileHover={{ y: -4 }}
              transition={{ type: 'spring', stiffness: 400, damping: 20 }}
            >
              <div className="lp-feature-card__icon"><Icon size={20} /></div>
              <h3 className="lp-feature-card__title">{title}</h3>
              <p className="lp-feature-card__desc">{desc}</p>
            </motion.div>
          ))}
        </div>
      </div>
    </section>
  );
}
