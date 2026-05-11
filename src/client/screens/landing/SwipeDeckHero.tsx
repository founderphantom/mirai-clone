import { useCallback, useEffect, useRef, useState } from 'react';
import {
  motion,
  useMotionValue,
  useTransform,
  AnimatePresence,
  animate,
  useReducedMotion,
} from 'motion/react';

const CARDS = [
  { src: '/landing/hero/hero-aesthetic-vibes.jpg',  label: 'Aesthetic Vibes' },
  { src: '/landing/hero/hero-retro-futurism.jpg',   label: 'Retro Futurism' },
  { src: '/landing/hero/hero-bali-vibes.jpg',        label: 'Bali Vibes' },
  { src: '/landing/hero/hero-grwm.jpg',              label: 'GRWM' },
  { src: '/landing/hero/hero-indie-aesthetic.jpg',   label: 'Indie Aesthetic' },
  { src: '/landing/hero/hero-cherry-blossom.jpg',    label: 'Cherry Blossom Seoul' },
  { src: '/landing/hero/hero-nyc-fashion.jpg',       label: 'NYC Fashion' },
];

type SwipeDirection = -1 | 0 | 1;
type CommittedSwipeDirection = Exclude<SwipeDirection, 0>;
type HeroCard = (typeof CARDS)[number];
type DepartingCard = HeroCard & {
  direction: CommittedSwipeDirection;
  id: number;
  initialRotate: number;
  initialX: number;
};

const AUTOPLAY_DELAY_MS = 3600;
const DRAG_RESET_SPRING = { type: 'spring', stiffness: 520, damping: 42 } as const;
const SWIPE_EXIT_SPRING = { type: 'spring', stiffness: 420, damping: 46 } as const;
const DECK_RESTACK_SPRING = {
  type: 'spring',
  stiffness: 360,
  damping: 44,
  mass: 0.9,
} as const;
const SWIPE_OFFSET_THRESHOLD = 90;
const SWIPE_VELOCITY_THRESHOLD = 500;
const SWIPE_EXIT_X = 480;
const SWIPE_EXIT_Y = 18;
const VISIBLE_STACK_CARDS = 4;

export function resolveSwipeDirection(offsetX: number, velocityX: number): SwipeDirection {
  const byVelocity = Math.abs(velocityX) > SWIPE_VELOCITY_THRESHOLD;
  const byOffset = Math.abs(offsetX) > SWIPE_OFFSET_THRESHOLD;

  if (!byVelocity && !byOffset) return 0;
  if (byVelocity) return velocityX < 0 ? -1 : 1;
  return offsetX < 0 ? -1 : 1;
}

export function getNextDeckIndex(
  activeIndex: number,
  _direction: CommittedSwipeDirection,
  total: number
) {
  if (total <= 0) return 0;
  return (activeIndex + 1) % total;
}

export function getDeckCardMotionState(position: number, total = CARDS.length) {
  const stackPosition = Math.min(position, VISIBLE_STACK_CARDS);
  const roundedScale = Number(Math.max(0.895, 1 - stackPosition * 0.035).toFixed(3));
  const roundedRotate =
    position === 0
      ? 0
      : Number((-2.5 + (stackPosition - 1) * 1.4).toFixed(1));

  return {
    x: position === 0 ? 0 : stackPosition * 12,
    y: position === 0 ? 0 : stackPosition * 10,
    rotate: roundedRotate,
    scale: roundedScale,
    opacity: position < VISIBLE_STACK_CARDS ? 1 : 0,
    zIndex: total - position,
  };
}

function DeckCard({
  card,
  isLocked,
  position,
  onDismiss,
  onInteractionStart,
  reduceMotion,
}: {
  card: HeroCard;
  isLocked: boolean;
  position: number;
  onDismiss: (
    direction: CommittedSwipeDirection,
    card: HeroCard,
    initialX: number,
    initialRotate: number
  ) => void;
  onInteractionStart: () => void;
  reduceMotion: boolean;
}) {
  const motionState = getDeckCardMotionState(position, CARDS.length);
  const isTopCard = position === 0;
  const x = useMotionValue(0);
  const rotate = useTransform(x, [-220, 220], [-10, 10]);

  useEffect(() => {
    x.set(0);
  }, [card.src, position, x]);

  return (
    <motion.div
      className="lp-deck__card"
      layout
      style={{
        position: 'absolute',
        zIndex: motionState.zIndex,
        pointerEvents: isTopCard && !isLocked ? 'auto' : 'none',
        ...(isTopCard ? { x, rotate } : {}),
      }}
      drag={isTopCard && !isLocked ? 'x' : false}
      dragConstraints={{ left: -280, right: 280 }}
      dragElastic={0.12}
      dragMomentum={false}
      onDragStart={() => {
        if (isTopCard) onInteractionStart();
      }}
      onDragEnd={(_, info) => {
        if (!isTopCard || isLocked) return;
        const direction = resolveSwipeDirection(info.offset.x, info.velocity.x);

        if (!direction) {
          animate(x, 0, DRAG_RESET_SPRING);
          return;
        }

        if (reduceMotion) {
          x.set(0);
          onDismiss(direction, card, 0, 0);
          return;
        }

        const initialX = x.get();
        const initialRotate = Math.max(-14, Math.min(14, (initialX / 220) * 10));
        x.set(0);
        onDismiss(direction, card, initialX, initialRotate);
      }}
      animate={
        isTopCard
          ? { y: motionState.y, scale: motionState.scale, opacity: motionState.opacity }
          : {
              x: motionState.x,
              y: motionState.y,
              rotate: motionState.rotate,
              scale: motionState.scale,
              opacity: motionState.opacity,
            }
      }
      transition={DECK_RESTACK_SPRING}
      whileDrag={{ scale: 1.025, cursor: 'grabbing' }}
    >
      <img
        src={card.src}
        alt={card.label}
        draggable={false}
        loading={isTopCard ? 'eager' : 'lazy'}
      />
    </motion.div>
  );
}

function DepartingDeckCard({
  card,
  onComplete,
}: {
  card: DepartingCard;
  onComplete: (id: number) => void;
}) {
  return (
    <motion.div
      className="lp-deck__card lp-deck__card--departing"
      initial={{
        x: card.initialX,
        y: 0,
        rotate: card.initialRotate,
        scale: 1,
        opacity: 1,
      }}
      animate={{
        x: card.direction * SWIPE_EXIT_X,
        y: SWIPE_EXIT_Y,
        rotate: card.direction * 16,
        scale: 1.02,
        opacity: 0,
      }}
      exit={{ opacity: 0 }}
      transition={SWIPE_EXIT_SPRING}
      style={{ position: 'absolute', zIndex: CARDS.length + 2 }}
      onAnimationComplete={() => onComplete(card.id)}
    >
      <img src={card.src} alt={card.label} draggable={false} loading="eager" />
    </motion.div>
  );
}

export function SwipeDeckHero() {
  const [activeIndex, setActiveIndex] = useState(0);
  const [departingCard, setDepartingCard] = useState<DepartingCard | null>(null);
  const autoplayTimeout = useRef<number | null>(null);
  const departingId = useRef(0);
  const isDismissing = useRef(false);
  const reduceMotion = useReducedMotion();

  const cancelAutoplay = useCallback(() => {
    if (autoplayTimeout.current === null) return;
    window.clearTimeout(autoplayTimeout.current);
    autoplayTimeout.current = null;
  }, []);

  const handleDismiss = useCallback((
    direction: CommittedSwipeDirection,
    card: HeroCard,
    initialX = 0,
    initialRotate = 0
  ) => {
    cancelAutoplay();
    if (isDismissing.current) return;
    isDismissing.current = true;

    if (!reduceMotion) {
      departingId.current += 1;
      setDepartingCard({
        ...card,
        direction,
        id: departingId.current,
        initialRotate,
        initialX,
      });
    } else {
      isDismissing.current = false;
    }

    setActiveIndex((index) => getNextDeckIndex(index, direction, CARDS.length));
  }, [cancelAutoplay, reduceMotion]);

  const clearDepartingCard = useCallback((id: number) => {
    if (departingId.current === id) {
      isDismissing.current = false;
    }
    setDepartingCard((current) => current?.id === id ? null : current);
  }, []);

  useEffect(() => {
    if (reduceMotion || departingCard || isDismissing.current) return;
    autoplayTimeout.current = window.setTimeout(() => {
      autoplayTimeout.current = null;
      handleDismiss(-1, CARDS[activeIndex], 0, 0);
    }, AUTOPLAY_DELAY_MS);
    return cancelAutoplay;
  }, [activeIndex, cancelAutoplay, departingCard, handleDismiss, reduceMotion]);

  const orderedCards = [
    ...CARDS.slice(activeIndex),
    ...CARDS.slice(0, activeIndex),
  ];

  return (
    <div className="lp-deck">
      {orderedCards.map((card, position) => (
        <DeckCard
          key={card.src}
          card={card}
          isLocked={Boolean(departingCard)}
          position={position}
          onDismiss={handleDismiss}
          onInteractionStart={cancelAutoplay}
          reduceMotion={Boolean(reduceMotion)}
        />
      ))}
      <AnimatePresence initial={false}>
        {departingCard ? (
          <DepartingDeckCard card={departingCard} onComplete={clearDepartingCard} />
        ) : null}
      </AnimatePresence>
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
