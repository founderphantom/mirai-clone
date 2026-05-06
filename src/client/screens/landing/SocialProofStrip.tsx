const HASHTAGS = [
  '#Y2KCafe',
  '#TokyoNeon',
  '#DarkAcademia',
  '#Cottagecore',
  '#CoastalGirl',
  '#Streetwear',
  '#Barbiecore',
  '#MoodyForest',
  '#Retrofuturism',
  '#GoldenHour',
];

export function SocialProofStrip() {
  return (
    <section className="lp-proof">
      <p className="lp-proof__label">Trusted by 10,000+ creators</p>
      <div className="lp-marquee">
        <div className="lp-marquee__track">
          {[...HASHTAGS, ...HASHTAGS].map((tag, i) => (
            <span key={i} className="lp-marquee__chip">{tag}</span>
          ))}
        </div>
      </div>
    </section>
  );
}
