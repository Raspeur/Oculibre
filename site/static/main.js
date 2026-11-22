// The only script of the site: the main button offers the download for the visitor's system.
// Without it the button simply leads to the download section.
(() => {
  const ua = navigator.userAgent;
  const platform = (navigator.userAgentData && navigator.userAgentData.platform) || "";
  const os = /Android/i.test(ua) ? "android"
    : /Win/i.test(platform || ua) ? "windows"
    : /Linux|X11/i.test(platform || ua) && !/CrOS/i.test(ua) ? "linux"
    : null;
  if (!os) return;
  const names = { windows: "Windows", linux: "Linux", android: "Android" };
  const main = document.getElementById("dl-main");
  const all = document.getElementById("dl-all");
  const card = document.getElementById("dl-" + os);
  if (!main) return;
  main.textContent = main.dataset.label.replace("{os}", names[os]);
  main.href = os === "linux" ? "#dl-linux" : main.dataset[os];
  if (all) all.hidden = false;
  if (card) card.classList.add("is-mine");
})();
