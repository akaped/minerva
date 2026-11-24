import { $t } from "./i18n.js";

const portraits = [
  ["fox", "🦊", () => $t("Fox")],
  ["frog", "🐸", () => $t("Frog")],
  ["robot", "🤖", () => $t("Robot")],
  ["alien", "👽", () => $t("Alien")],
  ["octopus", "🐙", () => $t("Octopus")],
  ["owl", "🦉", () => $t("Owl")],
];

export function profileAvatar(profile, name, esc) {
  const avatar = profile?.avatar || "";
  const preset = portraits.find(([key]) => avatar === `preset:${key}`);
  if (preset)
    return `<span class="profile-picture portrait-${preset[0]}" aria-hidden="true">${preset[1]}</span>`;
  if (/^data:image\/png;base64,[A-Za-z0-9+/=]+$/.test(avatar))
    return `<img class="profile-picture" src="${esc(avatar)}" alt="">`;
  const initials = (name || "").trim().split(/\s+/).slice(0, 2)
    .map((part) => Array.from(part)[0] || "").join("").toLocaleUpperCase();
  return `<span class="profile-picture profile-initials" aria-hidden="true">${esc(initials)}</span>`;
}

export function profileFields(profile, name, esc) {
  const options = portraits.map(([key, glyph, label]) => {
    const value = `preset:${key}`;
    return `<button type="button" class="avatar-choice" data-avatar="${value}" aria-label="${esc(label())}" aria-pressed="${profile.avatar === value}"><span class="profile-picture portrait-${key}" aria-hidden="true">${glyph}</span></button>`;
  }).join("");
  return `<div class="profile-fields">
    <div class="profile-name-fields"><label>${$t("First name")}<input name="first_name" autocomplete="given-name" required maxlength="200" value="${esc(profile.first_name || "")}"></label><label>${$t("Last name")}<input name="last_name" autocomplete="family-name" maxlength="200" value="${esc(profile.last_name || "")}"></label></div>
    <fieldset class="avatar-options"><legend>${$t("Profile picture")}</legend><div class="avatar-picker">
      <div class="avatar-preview" aria-hidden="true">${profileAvatar(profile, name, esc)}</div>
      <div class="avatar-picker-controls"><div class="avatar-choices">${options}</div><div class="avatar-actions"><button class="btn" type="button" id="choose-profile-photo">${$t("Upload your own photo")}</button><input type="file" accept="image/png,image/jpeg,image/webp" id="profile-upload" hidden><button class="btn" type="button" data-avatar="" aria-pressed="${!profile.avatar}">${$t("Use initials")}</button></div></div>
    </div></fieldset><input type="hidden" name="avatar" value="${esc(profile.avatar || "")}"><p class="avatar-feedback" role="status"></p>
  </div>`;
}

async function avatarFromFile(file) {
  if (file.size > 10 * 1024 * 1024 || !/^image\/(png|jpeg|webp)$/.test(file.type))
    throw new Error($t("Choose a PNG, JPEG, or WebP image up to 10 MB."));
  let image;
  try {
    image = await createImageBitmap(file);
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 128;
    const size = Math.min(image.width, image.height);
    canvas.getContext("2d").drawImage(
      image, (image.width - size) / 2, (image.height - size) / 2,
      size, size, 0, 0, 128, 128,
    );
    return canvas.toDataURL("image/png");
  } catch {
    throw new Error($t("This photo could not be opened."));
  } finally {
    image?.close();
  }
}

export function bindProfileFields(form, esc) {
  const preview = form.querySelector(".avatar-preview");
  const photo = form.querySelector("#profile-upload");
  form.querySelector("#choose-profile-photo").onclick = () => photo.click();
  const status = form.querySelector(".avatar-feedback");
  const avatar = form.elements.avatar;
  const save = form.querySelector('[type="submit"]');
  const choices = [...form.querySelectorAll("[data-avatar]")];
  let upload = 0;
  const paint = () => {
    const name = [form.elements.first_name.value, form.elements.last_name.value].join(" ");
    preview.innerHTML = profileAvatar({ avatar: avatar.value }, name, esc);
    for (const button of choices)
      button.setAttribute("aria-pressed", String(button.dataset.avatar === avatar.value));
  };
  for (const button of choices)
    button.onclick = () => {
      upload++;
      photo.value = "";
      avatar.value = button.dataset.avatar;
      save.disabled = false;
      status.textContent = "";
      paint();
    };
  form.elements.first_name.addEventListener("input", paint);
  form.elements.last_name.addEventListener("input", paint);
  photo.onchange = async () => {
    const file = photo.files[0];
    if (!file) return;
    const attempt = ++upload;
    status.textContent = "";
    save.disabled = true;
    try {
      const result = await avatarFromFile(file);
      if (attempt !== upload || !form.isConnected) return;
      avatar.value = result;
      paint();
    } catch (err) {
      if (attempt === upload) status.textContent = err.message;
    } finally {
      if (attempt === upload) save.disabled = false;
    }
  };
}
