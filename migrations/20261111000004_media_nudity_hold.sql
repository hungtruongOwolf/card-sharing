-- T-73: nudity classifier verdicts (ADR-0005, AC-28.3, D1). The model id and score of every
-- classified image are kept for audit; a flagged image is `held` and stays in quarantine.

ALTER TABLE media.media_objects
    ADD COLUMN classifier_score REAL NULL CHECK (classifier_score BETWEEN 0 AND 1),
    ADD COLUMN classifier_model TEXT NULL CHECK (length(classifier_model) <= 64),
    ADD CONSTRAINT media_objects_classifier_pair
        CHECK ((classifier_score IS NULL) = (classifier_model IS NULL));
