import { motion } from 'motion/react';

const STEPS = [
  {
    num: '01',
    img: '/landing/step-clone.jpg',
    title: 'Clone your look',
    desc: 'Paste your Instagram or upload 5+ photos. Our AI trains on your face in minutes.',
  },
  {
    num: '02',
    img: '/landing/step-moodboards.jpg',
    title: 'Pick your aesthetics',
    desc: 'Choose from 32 curated style moodboards. We build your personal inspiration pool.',
  },
  {
    num: '03',
    img: '/landing/step-blitz.jpg',
    title: 'Wake up to Blitz drops',
    desc: 'Every morning, fresh AI images of you in new aesthetics. Just swipe to save.',
  },
  {
    num: '04',
    img: '/landing/step-export.jpg',
    title: 'Export and post',
    desc: 'Reel 9:16, Story, Post 4:5 — all sized and ready. One tap to share.',
  },
];

export function HowItWorksSection() {
  return (
    <section className="lp-section" id="how-it-works">
      <div className="lp-container">
        <h2 className="lp-h2">From your face to viral content in four steps</h2>
        <div className="lp-steps">
          {STEPS.map((step, i) => (
            <motion.div
              key={step.num}
              className="lp-step"
              initial={{ opacity: 0, y: 24 }}
              whileInView={{ opacity: 1, y: 0 }}
              transition={{ delay: i * 0.08 }}
              viewport={{ once: true }}
            >
              <p className="lp-step__number">{step.num}</p>
              <img className="lp-step__img" src={step.img} alt={step.title} loading="lazy" />
              <h3 className="lp-step__title">{step.title}</h3>
              <p className="lp-step__desc">{step.desc}</p>
            </motion.div>
          ))}
        </div>
      </div>
    </section>
  );
}
