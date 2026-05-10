export type ReferenceValidation = {
  valid: boolean;
  message: string;
};

export const MIN_REFERENCE_PHOTOS = 5;
export const MAX_REFERENCE_PHOTOS = 20;
export const MAX_REFERENCE_PHOTO_BYTES = 15 * 1024 * 1024;

export const REFERENCE_GUIDANCE = [
  "One person per photo",
  "Clear face and hairline",
  "Varied angles and lighting",
  "Avoid heavy filters or sunglasses"
];

export const REFERENCE_EXAMPLES = [
  {
    label: "Fashion",
    caption: "Outfit and posture",
    src: "/landing/onboarding-upload/example-fashion.png"
  },
  {
    label: "Beauty",
    caption: "Face details",
    src: "/landing/onboarding-upload/example-beauty.png"
  },
  {
    label: "Travel",
    caption: "Natural light",
    src: "/landing/onboarding-upload/example-travel.png"
  },
  {
    label: "Content",
    caption: "Creator context",
    src: "/landing/onboarding-upload/example-content.png"
  },
  {
    label: "Vibes",
    caption: "Style range",
    src: "/landing/onboarding-upload/example-vibes.png"
  }
];

export function validateReferenceFiles(files: File[]): ReferenceValidation {
  if (files.length < MIN_REFERENCE_PHOTOS) {
    return {
      valid: false,
      message: "Choose at least 5 reference photos."
    };
  }

  if (files.length > MAX_REFERENCE_PHOTOS) {
    return {
      valid: false,
      message: "Choose no more than 20 reference photos."
    };
  }

  if (files.some((file) => !file.type.startsWith("image/"))) {
    return {
      valid: false,
      message: "Reference uploads must be image files."
    };
  }

  if (files.some((file) => file.size > MAX_REFERENCE_PHOTO_BYTES)) {
    return {
      valid: false,
      message: "Each reference photo must be 15 MB or smaller."
    };
  }

  return {
    valid: true,
    message: `${files.length} photos ready.`
  };
}
