/* global React, ReactDOM, Nav, Hero, Features, Philosophy, Showcase, Keyboard, Compare, QuickStart, FAQ, ReleasePeek, Foot, TweaksPanel, useTweaks, TweakSection, TweakRadio, TweakSelect, TweakSlider */
const { useState, useEffect } = React;

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "lang": "zh",
  "accent": "ridge",
  "density": "comfortable",
  "grid": "default"
}/*EDITMODE-END*/;

function App() {
  const [tweaks, setTweak] = useTweaks(TWEAK_DEFAULTS);

  useEffect(() => {
    const cls = document.body.classList;
    cls.toggle('compact', tweaks.density === 'compact');
    cls.toggle('accent-soil', tweaks.accent === 'soil');
    cls.toggle('accent-sun', tweaks.accent === 'sun');
    cls.toggle('grid-low', tweaks.grid === 'low');
    cls.toggle('grid-high', tweaks.grid === 'high');
  }, [tweaks]);

  // reveal-on-scroll
  useEffect(() => {
    const els = document.querySelectorAll('.reveal');
    const io = new IntersectionObserver((entries) => {
      entries.forEach(e => { if (e.isIntersecting) e.target.classList.add('in'); });
    }, { threshold: 0.1 });
    els.forEach(el => io.observe(el));
    return () => io.disconnect();
  }, [tweaks.lang]);

  const lang = tweaks.lang;

  return (
    <>
      <div className="ridge-grid-bg" aria-hidden="true"></div>
      <Nav lang={lang} setLang={(v) => setTweak('lang', v)} />
      <main className="page">
        <Hero lang={lang} />
        <Features lang={lang} />
        <Philosophy lang={lang} />
        <Showcase lang={lang} />
        <Keyboard lang={lang} />
        <Compare lang={lang} />
        <QuickStart lang={lang} />
        <ReleasePeek lang={lang} />
        <FAQ lang={lang} />
      </main>
      <Foot lang={lang} />

      <TweaksPanel title="Tweaks">
        <TweakSection title="Language · 语言">
          <TweakRadio
            value={tweaks.lang}
            onChange={v => setTweak('lang', v)}
            options={[{value:'zh', label:'中文'}, {value:'en', label:'English'}]}
          />
        </TweakSection>
        <TweakSection title="Accent">
          <TweakRadio
            value={tweaks.accent}
            onChange={v => setTweak('accent', v)}
            options={[{value:'ridge', label:'Ridge'}, {value:'soil', label:'Soil'}, {value:'sun', label:'Sun'}]}
          />
        </TweakSection>
        <TweakSection title="Density">
          <TweakRadio
            value={tweaks.density}
            onChange={v => setTweak('density', v)}
            options={[{value:'comfortable', label:'Comfortable'}, {value:'compact', label:'Compact'}]}
          />
        </TweakSection>
        <TweakSection title="Grid backdrop">
          <TweakRadio
            value={tweaks.grid}
            onChange={v => setTweak('grid', v)}
            options={[{value:'low', label:'Low'}, {value:'default', label:'Default'}, {value:'high', label:'High'}]}
          />
        </TweakSection>
      </TweaksPanel>
    </>
  );
}

ReactDOM.createRoot(document.getElementById('root')).render(<App />);
