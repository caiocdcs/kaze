-- Present while the API key kaze generated on its first start has not been
-- handed to a browser yet; the first claim deletes it.
CREATE TABLE unclaimed_api_key (
    id INTEGER PRIMARY KEY CHECK (id = 1)
);
