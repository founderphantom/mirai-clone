import { useState, useEffect } from 'react';
import { motion, useMotionValue, useTransform, AnimatePresence } from 'motion/react';

const CARDS = [
  { src: '/landing/hero/hero-aesthetic-vibes.jpg',  label: 'Aesthetic Vibes' },
  { src: '/landing/hero/hero-retro-futurism.jpg',   label: 'Retro Futurism' },
  { src: '/landing/hero/hero-bali-vibes.jpg',        label: 'Bali Vibes' },
  { src: '/landing/hero/hero-grwm.jpg',              label: 'GRWM' },
  { src: '/landing/hero/hero-indie-aesthetic.jpg',   label: 'Indie Aesthetic' },
  { src: '/landing/hero/hero-cherry-blossom.jpg',    label: 'Cherry Blossom Seoul' },
  { src: '/landing/hero/hero-nyc-fashion.jpg',       label: 'NYC Fashion' },
];

function DeckCard({
  src,
  label,
  position,
  onDragEnd,
}: {
  src: string;
  label: string;
  position: number;
  onDragEnd: (offsetX: number, velocityX: number) => void;
}) {
  const x = useMotionValue(0);
  const rotate = useTransform(x, [-200, 200], [-12, 12]);

  return (
    <motion.div
      className="lp-deck__card"
      style={{
        x,
        rotate: position === 0 ? rotate : position * 3 - 3,
        y: position * 8,
        zIndex: CARDS.length - position,
        position: 'absolute',
      }}
      drag={position === 0 ? 'x' : false}
      dragConstraints={{ left: -300, right: 300 }}
      dragElastic={0.15}
      dragMomentum={false}
      onDragEnd={(_, info) => {
        if (position === 0) onDragEnd(info.offset.x, info.velocity.x);
      }}
      animate={
        position !== 0
          ? { rotate: position * 3 - 3, y: position * 8, x: 0 }
          : undefined
      }
      transition={{ type: 'spring', stiffness: 400, damping: 35 }}
      whileDrag={{ scale: 1.02, cursor: 'grabbing' }}
    >
      <img
        src={src}
        alt={label}
        loading={position === 0 ? 'eager' : 'lazy'}
      />
    </motion.div>
  );
}

export function SwipeDeckHero() {
  const [activeIndex, setActiveIndex] = useState(0);

  useEffect(() => {
    const id = setInterval(() => {
      setActiveIndex((i) => (i + 1) % CARDS.length);
    }, 3000);
    return () => clearInterval(id);
  }, []);

  const handleDragEnd = (offsetX: number, velocityX: number) => {
    const byVelocity = Math.abs(velocityX) > 400;
    const byOffset = Math.abs(offsetX) > 80;
    if (byVelocity || byOffset) {
      if (velocityX < 0 || offsetX < 0) {
        setActiveIndex((i) => (i + 1) % CARDS.length);
      } else {
        setActiveIndex((i) => (i - 1 + CARDS.length) % CARDS.length);
      }
    }
  };

  const orderedCards = [
    ...CARDS.slice(activeIndex),
    ...CARDS.slice(0, activeIndex),
  ];

  return (
    <div className="lp-deck">
      {orderedCards.map((card, position) => (
        <DeckCard
          key={card.src}
          src={card.src}
          label={card.label}
          position={position}
          onDragEnd={handleDragEnd}
        />
      ))}
      <AnimatePresence mode="wait">
        <motion.div
          key={orderedCards[0].label}
          className="lp-deck__label"
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -4 }}
          transition={{ duration: 0.2 }}
        >
          {orderedCards[0].label}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
