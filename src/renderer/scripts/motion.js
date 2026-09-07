import { gsap } from "gsap";

const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
const active = new Set();
function track(tween) {
  active.add(tween);
  tween.eventCallback("onComplete", () => active.delete(tween));
  tween.eventCallback("onInterrupt", () => active.delete(tween));
}
function finishMotion() {
  for (const tween of [...active]) tween.progress(1);
  active.clear();
}
const onMotionPreference = () => {
  if (reducedMotion.matches) finishMotion();
};
reducedMotion.addEventListener("change", onMotionPreference);

export function animateView(view) {
  finishMotion();
  if (reducedMotion.matches || !view) return;
  // Only animate headings and controls; image comparison coordinates stay stable.
  const targets = [document.querySelector(".context-copy"),
    ...view.querySelectorAll(":scope > .stage-heading, :scope > .drop-zone, :scope > .selection-panel, :scope > .anime-toolbar, :scope > .page-toolbar, .settings-section"),
    ...document.querySelectorAll(".inspector:not(.hidden) .inspector-title")].filter(Boolean);
  if (!targets.length) return;
  track(gsap.fromTo(targets, { autoAlpha: 0, y: 10 }, {
    autoAlpha: 1, y: 0, duration: 0.32, stagger: 0.035,
    ease: "power2.out", overwrite: true, clearProps: "opacity,visibility,transform"
  }));
}

export function toggleGroup(button) {
  const content = button.nextElementSibling;
  if (!content) return;
  const opening = button.getAttribute("aria-expanded") === "false";
  gsap.killTweensOf(content);
  const height = content.getBoundingClientRect().height;
  button.setAttribute("aria-expanded", String(opening));
  content.inert = !opening;
  if (reducedMotion.matches) {
    gsap.set(content, { clearProps: "height,opacity,overflow,display" });
    return;
  }
  gsap.set(content, { display: "block", overflow: "hidden", height: "auto" });
  const targetHeight = content.getBoundingClientRect().height;
  track(gsap.fromTo(content, { height, opacity: opening ? 0 : 1 }, {
    height: opening ? targetHeight : 0, opacity: opening ? 1 : 0,
    duration: 0.24, ease: "power2.inOut",
    clearProps: "height,opacity,overflow,display"
  }));
}

if (import.meta.hot) import.meta.hot.dispose(() => {
  finishMotion();
  reducedMotion.removeEventListener("change", onMotionPreference);
});
