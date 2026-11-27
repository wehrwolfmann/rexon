// Rexon screensaver for the Steam Big Picture idle screen.
//
// The settings and the picture list come from manifest.js
// (window.REXON_SCREENSAVER), which Rexon writes on install. The page has
// no disk access: all pictures are downscaled copies in this same folder,
// the addresses are relative. No manifest — the bouncing Rexon logo.
//
// Spare the GPU: slideshow frames change by opacity, the zoom is one
// CSS transition over the whole frame, the logo moves with a transform from
// requestAnimationFrame; while the page is hidden, nothing is computed.
//
// For checking in a browser the settings can be overridden with address parameters:
// ?mode=bounce|slideshow&logo=rexon|games&clock=0|1&interval=<s>&at=<epoch ms>
"use strict";

(function () {
  const manifest = window.REXON_SCREENSAVER || {};
  const query = new URLSearchParams(window.location.search);

  function pick(name, fallback) {
    const v = query.get(name);
    if (v !== null && v !== "") return v;
    return manifest[name] !== undefined ? manifest[name] : fallback;
  }

  const slides = Array.isArray(manifest.slides) ? manifest.slides.filter(s => s && s.image) : [];
  const gameLogos = Array.isArray(manifest.logos) ? manifest.logos.filter(Boolean) : [];
  const rexonLogo = manifest.rexonLogo || "rexon-logo.png";
  let mode = String(pick("mode", "slideshow"));
  if (mode === "slideshow" && slides.length === 0) mode = "bounce";
  let logoChoice = String(pick("logo", "rexon"));
  if (logoChoice === "games" && gameLogos.length === 0) logoChoice = "rexon";
  const intervalS = Math.min(60, Math.max(4, Number(pick("interval", 9)) || 9));
  const clockOn = String(pick("clock", true)) !== "0" && pick("clock", true) !== false;
  const locale = manifest.locale || navigator.language || "en";
  // Overriding «now» — only for check snapshots.
  const fixedNow = Number(query.get("at")) || 0;

  const FADE_S = 1.6;

  // ── Clock ─────────────────────────────────────────────────────────────
  const clock = document.getElementById("clock");
  const timeFmt = new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit" });
  const dateFmt = new Intl.DateTimeFormat(locale, { weekday: "long", day: "numeric", month: "long" });
  const shotFmt = new Intl.DateTimeFormat(locale, { day: "numeric", month: "long", year: "numeric" });

  function now() { return fixedNow ? new Date(fixedNow) : new Date(); }

  function tickClock() {
    const d = now();
    clock.querySelector(".time").textContent = timeFmt.format(d);
    clock.querySelector(".date").textContent = dateFmt.format(d);
    // The next time — exactly at the minute change, not once a second.
    const wait = 60000 - (d.getSeconds() * 1000 + d.getMilliseconds()) + 50;
    if (!fixedNow) setTimeout(tickClock, wait);
  }
  if (clockOn) {
    clock.style.display = "block";
    tickClock();
  }

  // ── Slideshow ─────────────────────────────────────────────────────────
  function cssUrl(name) {
    return 'url("' + encodeURI(name).replace(/"/g, "%22") + '")';
  }

  function preload(name) {
    return new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => {
        if (img.decode) img.decode().then(resolve, resolve); else resolve();
      };
      img.onerror = reject;
      img.src = name;
    });
  }

  function runSlideshow() {
    const layers = [document.getElementById("slideA"), document.getElementById("slideB")];
    let front = 0;
    let index = -1;
    let timer = 0;
    let failures = 0;

    function fill(layer, slide, flip) {
      const pan = layer.querySelector(".pan");
      const fr = layer.querySelector(".front");
      const cap = layer.querySelector(".caption");
      layer.classList.remove("moving", "reverse", "framed", "with-logo");
      pan.style.transition = "none";
      if (slide.framed) {
        // A vertical cover: the background is a blurred copy, the cover itself in the center.
        pan.style.backgroundImage = cssUrl(slide.backdrop || slide.image);
        fr.style.backgroundImage = cssUrl(slide.image);
        layer.classList.add("framed");
      } else {
        pan.style.backgroundImage = cssUrl(slide.image);
        if (slide.logo) {
          fr.style.backgroundImage = cssUrl(slide.logo);
          layer.classList.add("with-logo");
        } else {
          fr.style.backgroundImage = "none";
        }
      }
      cap.textContent = "";
      if (slide.kind === "shot" && (slide.title || slide.when)) {
        const t = document.createElement("span");
        t.textContent = slide.title || "";
        cap.appendChild(t);
        if (slide.when) {
          const w = document.createElement("span");
          w.className = "when";
          w.textContent = shotFmt.format(new Date(slide.when * 1000));
          cap.appendChild(w);
        }
      }
      if (flip) layer.classList.add("reverse");
      // Reset the zoom without animation, then zoom over the whole frame length.
      void pan.offsetWidth;
      pan.style.transition = "transform " + (intervalS + FADE_S * 2) + "s linear";
    }

    function wanted(slide) {
      return [slide.image, slide.logo, slide.framed ? slide.backdrop : null].filter(Boolean);
    }

    function next() {
      timer = 0;
      if (document.hidden) return;
      index = (index + 1) % slides.length;
      const slide = slides[index];
      Promise.all(wanted(slide).map(preload)).then(() => {
        failures = 0;
        const back = 1 - front;
        const layer = layers[back];
        fill(layer, slide, index % 2 === 1);
        requestAnimationFrame(() => {
          layer.classList.add("shown", "moving");
          layers[front].classList.remove("shown");
          front = back;
          timer = setTimeout(next, intervalS * 1000);
        });
      }, () => {
        // The picture cannot be opened — the next frame; if nothing opens,
        // we switch to the logo instead of spinning idle.
        failures += 1;
        if (failures >= slides.length) {
          layers.forEach(l => l.classList.remove("shown"));
          runBounce();
          return;
        }
        timer = setTimeout(next, 50);
      });
    }

    document.addEventListener("visibilitychange", () => {
      if (!document.hidden && !timer) next();
    });
    next();
  }

  // ── Bouncing logo ─────────────────────────────────────────────────
  // The logo flies at a constant speed, bounces off the edge and changes its look:
  // the Rexon logo — its tint, in the «game logos» mode — the next game.
  const HUES = [0, 70, 150, 205, 265, 320];

  function runBounce() {
    document.body.classList.add("bounce");
    const el = document.getElementById("bouncer");
    const pool = logoChoice === "games" ? gameLogos.slice() : [rexonLogo];
    let poolIndex = Math.floor(Math.random() * pool.length);
    let hue = 0;
    let w = 0, h = 0, x = 0, y = 0, vx = 0, vy = 0, last = 0, raf = 0;

    function size() {
      const W = window.innerWidth, H = window.innerHeight;
      const box = Math.min(W, H) * (logoChoice === "games" ? 0.34 : 0.26);
      const nw = el.naturalWidth || 1, nh = el.naturalHeight || 1;
      const k = Math.min(box / nw, box / nh) * (logoChoice === "games" ? 1.4 : 1);
      w = Math.round(nw * k);
      h = Math.round(nh * k);
      el.style.width = w + "px";
      el.style.height = h + "px";
      // The logo crosses the smaller side of the screen in about 12 seconds.
      const speed = Math.min(W, H) / 12;
      const len = Math.hypot(vx, vy) || 1;
      vx = vx / len * speed;
      vy = vy / len * speed;
      x = Math.min(Math.max(0, x), Math.max(0, W - w));
      y = Math.min(Math.max(0, y), Math.max(0, H - h));
    }

    function start() {
      const W = window.innerWidth, H = window.innerHeight;
      const a = (Math.PI / 6) + Math.random() * (Math.PI / 6) + Math.floor(Math.random() * 4) * (Math.PI / 2);
      vx = Math.cos(a);
      vy = Math.sin(a);
      size();
      x = (W - w) * (0.2 + Math.random() * 0.6);
      y = (H - h) * (0.2 + Math.random() * 0.6);
    }

    function restyle() {
      if (logoChoice === "games") {
        poolIndex = (poolIndex + 1) % pool.length;
        const keep = { x, y };
        el.onload = () => { size(); x = keep.x; y = keep.y; };
        el.src = pool[poolIndex];
      } else {
        hue = (hue + 1) % HUES.length;
        el.style.filter = HUES[hue] ? "hue-rotate(" + HUES[hue] + "deg)" : "none";
      }
    }

    function frame(t) {
      raf = 0;
      if (document.hidden) { last = 0; return; }
      const dt = last ? Math.min(0.1, (t - last) / 1000) : 0;
      last = t;
      const W = window.innerWidth, H = window.innerHeight;
      x += vx * dt;
      y += vy * dt;
      let hits = 0;
      if (x <= 0) { x = 0; vx = Math.abs(vx); hits++; }
      else if (x + w >= W) { x = Math.max(0, W - w); vx = -Math.abs(vx); hits++; }
      if (y <= 0) { y = 0; vy = Math.abs(vy); hits++; }
      else if (y + h >= H) { y = Math.max(0, H - h); vy = -Math.abs(vy); hits++; }
      if (hits) restyle();
      el.style.transform = "translate3d(" + x.toFixed(1) + "px," + y.toFixed(1) + "px,0)";
      raf = requestAnimationFrame(frame);
    }

    el.onload = () => {
      el.onload = null;
      el.style.display = "block";
      start();
      el.style.transform = "translate3d(" + x.toFixed(1) + "px," + y.toFixed(1) + "px,0)";
      if (!raf) raf = requestAnimationFrame(frame);
    };
    el.onerror = () => {
      // The game logo did not open — our own logo is always there.
      if (el.src.indexOf(rexonLogo) < 0) { logoChoice = "rexon"; el.src = rexonLogo; }
    };
    el.src = pool[poolIndex];

    window.addEventListener("resize", size);
    document.addEventListener("visibilitychange", () => {
      if (!document.hidden && !raf) raf = requestAnimationFrame(frame);
    });
  }

  if (mode === "bounce") runBounce(); else runSlideshow();
})();
