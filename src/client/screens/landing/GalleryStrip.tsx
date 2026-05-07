import { useRef, useState, useEffect } from 'react';
import { GALLERY_NICHES } from '../../../data/landing-niches';

function useLazyImage(src: string, rootMargin = '300px') {
  const ref = useRef<HTMLDivElement>(null);
  const [resolvedSrc, setResolvedSrc] = useState<string | undefined>();

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setResolvedSrc(src);
          observer.disconnect();
        }
      },
      { rootMargin }
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [src, rootMargin]);

  return { ref, resolvedSrc };
}

const nicheToSlug = (label: string) =>
  label.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '');

const ROW1 = GALLERY_NICHES.filter((_, i) => i % 2 === 0).map((n) => ({
  src: `/landing/gallery/gallery-${nicheToSlug(n.label)}.jpg`,
  label: n.label,
}));

const ROW2 = GALLERY_NICHES.filter((_, i) => i % 2 === 1).map((n) => ({
  src: `/landing/gallery/gallery-${nicheToSlug(n.label)}.jpg`,
  label: n.label,
}));

function GalleryCard({ src, label }: { src: string; label: string }) {
  const { ref, resolvedSrc } = useLazyImage(src, '300px');

  return (
    <div className="lp-gallery__card" ref={ref}>
      {resolvedSrc ? (
        <img src={resolvedSrc} alt={label} loading="lazy" />
      ) : (
        <div className="lp-gallery__card-placeholder" aria-hidden="true" />
      )}
      <div className="lp-gallery__card-label">{label}</div>
    </div>
  );
}

export function GalleryStrip() {
  return (
    <section className="lp-section" id="gallery">
      <div className="lp-container">
        <h2 className="lp-h2">See what Mirai makes</h2>
      </div>
      <div className="lp-gallery">
        <div className="lp-gallery__row">
          <div className="lp-gallery__track">
            {[...ROW1, ...ROW1, ...ROW1].map((card, i) => (
              <GalleryCard key={i} {...card} />
            ))}
          </div>
        </div>
        <div className="lp-gallery__row">
          <div className="lp-gallery__track lp-gallery__track--reverse">
            {[...ROW2, ...ROW2, ...ROW2].map((card, i) => (
              <GalleryCard key={i} {...card} />
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}
