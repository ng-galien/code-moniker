import { ChangeReviewResult } from "./model";

export function update(review: ChangeReviewResult): number {
	return review.files;
}
