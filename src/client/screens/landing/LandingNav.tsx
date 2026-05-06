import { useState, useEffect } from 'react';

interface LandingNavProps {
  onGetStarted: () => void;
}

export function LandingNav({ onGetStarted }: LandingNavProps) {
  const [scrolled, setScrolled] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 80);
    window.addEventListener('scroll', onScroll, { passive: true });
    return () => window.removeEventListener('scroll', onScroll);
  }, []);

  const handleAnchor = (e: React.MouseEvent<HTMLAnchorElement>, hash: string) => {
    e.preventDefault();
    document.querySelector(hash)?.scrollIntoView({ behavior: 'smooth' });
    setDrawerOpen(false);
  };

  return (
    <>
      <nav className={`lp-nav${scrolled ? ' lp-nav--scrolled' : ''}`}>
        <div className="lp-nav__inner">
          <a className="lp-nav__logo" href="/">
            <img src="/icons/mirai-icon.svg" width={28} height={28} alt="" />
            Mirai
          </a>
          <ul className="lp-nav__links">
            <li><a href="#how-it-works" onClick={(e) => handleAnchor(e, '#how-it-works')}>How it Works</a></li>
            <li><a href="#gallery" onClick={(e) => handleAnchor(e, '#gallery')}>Gallery</a></li>
            <li><a href="#pricing" onClick={(e) => handleAnchor(e, '#pricing')}>Pricing</a></li>
          </ul>
          <div className="lp-nav__right">
            <a className="lp-nav__signin" href="/login">Sign in</a>
            <button className="lp-btn-primary lp-nav__cta" onClick={onGetStarted}>Get started free</button>
          </div>
          <button className="lp-nav__hamburger" onClick={() => setDrawerOpen(true)} aria-label="Open menu">
            <span /><span /><span />
          </button>
        </div>
      </nav>
      {drawerOpen && (
        <div className="lp-drawer-overlay" onClick={() => setDrawerOpen(false)}>
          <div className="lp-drawer" onClick={(e) => e.stopPropagation()}>
            <button className="lp-drawer__close" onClick={() => setDrawerOpen(false)} aria-label="Close menu">✕</button>
            <ul className="lp-drawer__links">
              <li><a href="#how-it-works" onClick={(e) => handleAnchor(e, '#how-it-works')}>How it Works</a></li>
              <li><a href="#gallery" onClick={(e) => handleAnchor(e, '#gallery')}>Gallery</a></li>
              <li><a href="#pricing" onClick={(e) => handleAnchor(e, '#pricing')}>Pricing</a></li>
            </ul>
            <a className="lp-nav__signin" href="/login" onClick={() => setDrawerOpen(false)}>Sign in</a>
            <button className="lp-btn-primary" onClick={() => { setDrawerOpen(false); onGetStarted(); }}>Get started free</button>
          </div>
        </div>
      )}
    </>
  );
}
