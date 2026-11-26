import type { InputHTMLAttributes } from "react";

export function TextInput(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className="h-[30px] min-w-0 w-full border border-gray-400 bg-background-100 px-3 text-label-13 text-gray-1000 placeholder:text-gray-700" />;
}
