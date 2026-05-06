export function LandingFooter() {
  return (
    <footer className="lp-footer">
      <div className="lp-container">
        <div className="lp-footer__grid">
          <div>
            <div className="lp-footer__brand">
              <img src="/icons/mirai-icon.svg" width={24} height={24} alt="" />
              Mirai
            </div>
            <p className="lp-footer__tagline">The fastest way to put yourself in every trend.</p>
          </div>
          <div>
            <p className="lp-footer__col-title">Product</p>
            <ul className="lp-footer__links">
              <li><a href="#how-it-works">How it Works</a></li>
              <li><a href="#gallery">Gallery</a></li>
              <li><a href="#pricing">Pricing</a></li>
              <li><a href="#">Changelog</a></li>
            </ul>
          </div>
          <div>
            <p className="lp-footer__col-title">Legal</p>
            <ul className="lp-footer__links">
              <li><a href="#">Privacy</a></li>
              <li><a href="#">Terms</a></li>
              <li><a href="#">Contact</a></li>
            </ul>
          </div>
        </div>
        <div className="lp-footer__bottom">© 2026 Mirai · Made for creators</div>
      </div>
    </footer>
  );
}
