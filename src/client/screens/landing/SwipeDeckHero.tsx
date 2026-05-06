import { useState, useEffect } from 'react';
import { motion, useMotionValue, useTransform } from 'motion/react';

const CARDS = [
  { src: '/landing/clone-y2k-cafe.jpg', label: 'Y2K Cafe' },
  { src: '/landing/clone-tokyo-neon.jpg', label: 'Tokyo Neon' },
  { src: '/landing/clone-cottagecore-picnic.jpg', label: 'Cottagecore' },
  { src: '/landing/clone-coastal-sunset.jpg', label: 'Coastal Sunset' },
  { src: '/landing/clone-dark-academia.jpg', label: 'Dark Academia' },
  { src: '/landing/clone-streetwear-berlin.jpg', label: 'Streetwear Berlin' },
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
  onDragEnd: (offset: number) => void;
}) {
  const x = useMotionValue(0);
  const rotate = useTransform(x, [-100, 100], [-8, 8]);

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
      dragConstraints={{ left: -100, right: 100 }}
      onDragEnd={(_, info) => {
        if (position === 0) onDragEnd(info.offset.x);
      }}
      animate={position !== 0 ? { rotate: position * 3 - 3, y: position * 8 } : undefined}
      transition={{ type: 'spring', stiffness: 300, damping: 30 }}
    >
      <img src={src} alt={label} loading="eager" />
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

  const handleDragEnd = (offset: number) => {
    if (offset < -50) {
      setActiveIndex((i) => (i + 1) % CARDS.length);
    } else if (offset > 50) {
      setActiveIndex((i) => (i - 1 + CARDS.length) % CARDS.length);
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
      <div className="lp-deck__label">{orderedCards[0].label}</div>
    </div>
  );
}
