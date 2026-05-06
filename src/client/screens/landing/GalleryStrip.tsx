const ROW1 = [
  { src: '/landing/clone-y2k-cafe.jpg', label: 'Y2K Cafe' },
  { src: '/landing/clone-dark-academia.jpg', label: 'Dark Academia' },
  { src: '/landing/clone-cottagecore-picnic.jpg', label: 'Cottagecore Picnic' },
  { src: '/landing/clone-coastal-sunset.jpg', label: 'Coastal Sunset' },
  { src: '/landing/clone-barbiecore.jpg', label: 'Barbiecore' },
  { src: '/landing/clone-cherry-blossom-seoul.jpg', label: 'Cherry Blossom Seoul' },
];

const ROW2 = [
  { src: '/landing/clone-tokyo-neon.jpg', label: 'Tokyo Neon' },
  { src: '/landing/clone-streetwear-berlin.jpg', label: 'Streetwear Berlin' },
  { src: '/landing/clone-moody-forest.jpg', label: 'Moody Forest' },
  { src: '/landing/clone-retrofuturism.jpg', label: 'Retrofuturism' },
  { src: '/landing/clone-golden-hour-desert.jpg', label: 'Golden Hour Desert' },
  { src: '/landing/clone-winter-minimalist.jpg', label: 'Winter Minimalist' },
];

function GalleryCard({ src, label }: { src: string; label: string }) {
  return (
    <div className="lp-gallery__card">
      <img src={src} alt={label} loading="lazy" />
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
            {[...ROW1, ...ROW1].map((card, i) => (
              <GalleryCard key={i} {...card} />
            ))}
          </div>
        </div>
        <div className="lp-gallery__row">
          <div className="lp-gallery__track lp-gallery__track--reverse">
            {[...ROW2, ...ROW2].map((card, i) => (
              <GalleryCard key={i} {...card} />
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}
