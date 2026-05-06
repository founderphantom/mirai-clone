import { motion } from 'motion/react';

const TESTIMONIALS = [
  {
    initial: 'S',
    name: 'Skyla M.',
    handle: '@skylavibes',
    followers: '84K followers',
    quote: 'I post every day now without burning out. Mirai does the creative work.',
    metric: '3× my posting frequency',
  },
  {
    initial: 'M',
    name: 'Marina C.',
    handle: '@marina.coastal',
    followers: '41K followers',
    quote: "The Blitz deck is actually addictive. I've saved 200 looks in two weeks.",
    metric: '200 saves in 2 weeks',
  },
  {
    initial: 'A',
    name: 'Aiden S.',
    handle: '@aiden.streets',
    followers: '29K followers',
    quote: 'I went from posting once a week to daily. My reach went up 180%.',
    metric: '+180% reach',
  },
  {
    initial: 'N',
    name: 'Nova A.',
    handle: '@nova.aesthetic',
    followers: '112K followers',
    quote: "Every morning I open Mirai before Instagram. It's become my ritual.",
    metric: 'Daily habit in week 1',
  },
];

export function TestimonialsSection() {
  return (
    <section className="lp-section">
      <div className="lp-container">
        <h2 className="lp-h2">Creators are already going viral</h2>
        <div className="lp-testimonials">
          <motion.div
            className="lp-testimonials__carousel"
            drag="x"
            dragConstraints={{ left: -1032, right: 0 }}
            dragElastic={0.1}
          >
            {TESTIMONIALS.map((t) => (
              <div key={t.handle} className="lp-testimonial-card">
                <div className="lp-testimonial-card__avatar">{t.initial}</div>
                <div className="lp-testimonial-card__stars">★★★★★</div>
                <p className="lp-testimonial-card__quote">"{t.quote}"</p>
                <p className="lp-testimonial-card__name">{t.name}</p>
                <p className="lp-testimonial-card__handle">{t.handle} · {t.followers}</p>
                <span className="lp-testimonial-card__metric">{t.metric}</span>
              </div>
            ))}
          </motion.div>
        </div>
      </div>
    </section>
  );
}
